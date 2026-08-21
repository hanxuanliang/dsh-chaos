//! Shared test fixture utilities; domain assertions stay with their modules.

pub(crate) use std::sync::Arc;

use turso::Connection;

pub(crate) use crate::*;

pub(crate) async fn fixture() -> Result<(Arc<CollabCore>, Actor, Actor, Actor, Target)> {
    let core = Arc::new(CollabCore::open_memory().await?);
    let user = core.create_user("owner", "Owner").await?;
    let alpha = core.create_agent("alpha", "Alpha", "/tmp/alpha").await?;
    let beta = core.create_agent("beta", "Beta", "/tmp/beta").await?;
    let channel = core.create_channel("design", &user.id).await?;
    core.add_member(&channel.id, &alpha.id, &user.id).await?;
    core.add_member(&channel.id, &beta.id, &user.id).await?;
    Ok((core, user, alpha, beta, channel))
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

pub(crate) async fn count_where(
    connection: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<i64> {
    let allowed = [
        ("memberships", "actor_id"),
        ("runtime_bindings", "agent_id"),
        ("agents", "actor_id"),
        ("agent_wake_state", "agent_id"),
        ("inbox_batches", "agent_id"),
        ("actors", "id"),
        ("messages", "author_id"),
    ];
    assert!(allowed.contains(&(table, column)));
    let mut rows = connection
        .query(
            format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
            [value],
        )
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
    Ok(row.get(0)?)
}
