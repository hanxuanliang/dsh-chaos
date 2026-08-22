//! Shared test fixture utilities; domain assertions stay with their modules.

pub(crate) use std::sync::Arc;

use turso::Connection;

pub(crate) use crate::*;

/// The shared collaboration world for cross-vertical tests: one in-memory
/// core with an owner, two Agents, and one Channel holding both Agents.
pub(crate) struct World {
    pub(crate) core: Arc<CollabCore>,
    pub(crate) user: Actor,
    pub(crate) alpha: Actor,
    pub(crate) beta: Actor,
    pub(crate) channel: Target,
}

impl World {
    pub(crate) async fn create() -> Result<Self> {
        let core = Arc::new(CollabCore::open_memory().await?);
        let user = core.create_user("owner", "Owner").await?;
        let alpha = core.create_agent("alpha", "Alpha", "/tmp/alpha").await?;
        let beta = core.create_agent("beta", "Beta", "/tmp/beta").await?;
        let channel = core.create_channel("design", &user.id).await?;
        core.add_member(&channel.id, &alpha.id, &user.id).await?;
        core.add_member(&channel.id, &beta.id, &user.id).await?;
        Ok(Self {
            core,
            user,
            alpha,
            beta,
            channel,
        })
    }
}

pub(crate) async fn count(connection: &Connection, table: &str) -> Result<i64> {
    let allowed = [
        "messages",
        "deliveries",
        "agent_wake_state",
        "inbox_batch_items",
        "actors",
    ];
    assert!(allowed.contains(&table));
    let mut rows = connection
        .query(format!("SELECT COUNT(*) FROM {table}"), ())
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
    Ok(row.get(0)?)
}
