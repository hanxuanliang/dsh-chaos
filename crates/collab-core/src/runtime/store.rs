//! Runtime binding lookups shared by the command, query, and delivery layers.

use turso::Connection;

use crate::db::QueryRows;
use crate::{CollabError, Result};

use super::model::RuntimeBinding;

/// Column order of the canonical RuntimeBinding projection, shared by every
/// SELECT in this vertical.
const BINDING_COLUMNS: &str =
    "agent_id, session_id, generation, provider, model, preset, bound_at_ms";

/// Load the current binding for one stable Agent.
pub(crate) async fn binding_for_agent(
    connection: &Connection,
    agent_id: &str,
) -> Result<Option<RuntimeBinding>> {
    connection
        .query_row::<RuntimeBinding>(
            &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings WHERE agent_id = ?1"),
            [agent_id],
        )
        .await
}

/// Resolve the stable Agent identity that owns one live DSH Session id.
pub(crate) async fn binding_for_session(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<RuntimeBinding>> {
    connection
        .query_row::<RuntimeBinding>(
            &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings WHERE session_id = ?1"),
            [session_id],
        )
        .await
}

/// List every durable current runtime binding for process recovery.
pub(crate) async fn all_bindings(connection: &Connection) -> Result<Vec<RuntimeBinding>> {
    connection
        .query_rows::<RuntimeBinding>(
            &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings ORDER BY agent_id"),
            (),
        )
        .await
}

/// The generation counter for one Agent; zero before the first bind.
pub(crate) async fn current_generation(connection: &Connection, agent_id: &str) -> Result<i64> {
    let mut rows = connection
        .query(
            "SELECT generation FROM runtime_bindings WHERE agent_id = ?1",
            [agent_id],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Ok(0),
    }
}

/// Fail unless the durable binding for `agent_id` is exactly the fenced
/// Session and generation pair.
pub(crate) async fn require_current_binding(
    connection: &Connection,
    agent_id: &str,
    generation: i64,
    session_id: &str,
) -> Result<()> {
    let mut rows = connection
        .query(
            "SELECT session_id, generation FROM runtime_bindings WHERE agent_id = ?1",
            [agent_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::NotFound {
            entity: "runtime binding",
            id: agent_id.to_owned(),
        });
    };
    let current_session = row.get::<String>(0)?;
    let current_generation = row.get::<i64>(1)?;
    if current_session != session_id || current_generation != generation {
        return Err(CollabError::RuntimeGenerationMismatch {
            agent_id: agent_id.to_owned(),
        });
    }
    Ok(())
}
