use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::Result;
use crate::db::FromRow;

/// The current DSH runtime generation bound to a stable Agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeBinding {
    pub agent_id: String,
    pub session_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub preset: String,
    pub bound_at_ms: i64,
}

impl FromRow for RuntimeBinding {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            agent_id: row.get(0)?,
            session_id: row.get(1)?,
            generation: row.get(2)?,
            provider: row.get(3)?,
            model: row.get(4)?,
            preset: row.get(5)?,
            bound_at_ms: row.get(6)?,
        })
    }
}

impl RuntimeBinding {
    /// Insert or replace the durable current binding for this Agent.
    pub(crate) async fn upsert(&self, connection: &Connection) -> Result<()> {
        connection
            .execute(
                "INSERT INTO runtime_bindings
                 (agent_id, session_id, generation, provider, model, preset, bound_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(agent_id) DO UPDATE SET
                   session_id = excluded.session_id,
                   generation = excluded.generation,
                   provider = excluded.provider,
                   model = excluded.model,
                   preset = excluded.preset,
                   bound_at_ms = excluded.bound_at_ms",
                (
                    self.agent_id.as_str(),
                    self.session_id.as_str(),
                    self.generation,
                    self.provider.as_str(),
                    self.model.as_str(),
                    self.preset.as_str(),
                    self.bound_at_ms,
                ),
            )
            .await?;
        Ok(())
    }

    /// Rewrite the preset label for exactly this Agent, Session, and
    /// generation.
    pub(crate) async fn update_preset(&self, connection: &Connection, preset: &str) -> Result<()> {
        connection
            .execute(
                "UPDATE runtime_bindings SET preset = ?1
                 WHERE agent_id = ?2 AND generation = ?3 AND session_id = ?4",
                (
                    preset,
                    self.agent_id.as_str(),
                    self.generation,
                    self.session_id.as_str(),
                ),
            )
            .await?;
        Ok(())
    }
}
