//! Stable Agent to DSH Session generation bindings.

use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::actor::{Actor, ActorId};
use crate::db::{FromRow, QueryRows};
use crate::{CollabCore, CollabError, Result, now_ms};

// ── 类型 ─────────────────────────────────────────────────────────────────────

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

// ── 证据 ─────────────────────────────────────────────────────────────────────

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

// ── 能力 ─────────────────────────────────────────────────────────────────────

impl CollabCore {
    /// Bind a new DSH Session generation to a stable Agent.
    pub async fn bind_runtime(
        &self,
        agent_id: &str,
        session_id: &str,
        provider: &str,
        model: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        let _ = ActorId::parse(agent_id)?;
        require_non_blank!(session_id, provider, model, preset);
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require_agent(connection, &ActorId::parse(agent_id)?).await?;
            let generation = RuntimeStore::new(connection)
                .current_generation(agent_id)
                .await?
                + 1;
            let binding = RuntimeBinding {
                agent_id: agent_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
                provider: provider.to_owned(),
                model: model.to_owned(),
                preset: preset.to_owned(),
                bound_at_ms: now,
            };
            RuntimeStore::new(connection).upsert(&binding).await?;
            Ok(binding)
        })
        .await
    }

    /// Replace only the preset label of one exact runtime generation.
    ///
    /// Used to migrate the former chaos `default` sentinel after the official
    /// DSH roster resolves it. Session identity, generation, and wake fencing
    /// stay unchanged.
    pub async fn update_runtime_preset(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        let _ = ActorId::parse(agent_id)?;
        require_non_blank!(session_id, preset);
        self.write(async |connection| {
            let current = RuntimeStore::new(connection)
                .binding_for_agent(agent_id)
                .await?
                .ok_or_else(|| CollabError::NotFound {
                    entity: "runtime binding",
                    id: agent_id.to_owned(),
                })?;
            if current.generation != generation || current.session_id != session_id {
                return Err(CollabError::RuntimeGenerationMismatch {
                    agent_id: agent_id.to_owned(),
                });
            }
            RuntimeStore::new(connection)
                .update_preset(&current, preset)
                .await?;
            Ok(RuntimeBinding {
                preset: preset.to_owned(),
                ..current
            })
        })
        .await
    }

    /// Return the current runtime binding for one stable Agent.
    pub async fn runtime_binding(&self, agent_id: &str) -> Result<Option<RuntimeBinding>> {
        let _ = ActorId::parse(agent_id)?;
        self.read(async |connection| {
            RuntimeStore::new(connection)
                .binding_for_agent(agent_id)
                .await
        })
        .await
    }

    /// Resolve the stable Agent identity that owns one live DSH Session id.
    pub async fn runtime_binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RuntimeBinding>> {
        let _ = ActorId::parse(session_id)?;
        self.read(async |connection| {
            RuntimeStore::new(connection)
                .binding_for_session(session_id)
                .await
        })
        .await
    }

    /// List every durable current runtime binding for process recovery.
    pub async fn list_runtime_bindings(&self) -> Result<Vec<RuntimeBinding>> {
        self.read(async |connection| RuntimeStore::new(connection).all_bindings().await)
            .await
    }
}

// ── 存储 ─────────────────────────────────────────────────────────────────────

/// Column order of the canonical RuntimeBinding projection, shared by every
/// SELECT in this vertical.
const BINDING_COLUMNS: &str =
    "agent_id, session_id, generation, provider, model, preset, bound_at_ms";

/// Persistence for the runtime_bindings table; the only owner of its SQL.
pub(crate) struct RuntimeStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> RuntimeStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Load the current binding for one stable Agent.
    pub(crate) async fn binding_for_agent(&self, agent_id: &str) -> Result<Option<RuntimeBinding>> {
        self.connection
            .query_row::<RuntimeBinding>(
                &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings WHERE agent_id = ?1"),
                [agent_id],
            )
            .await
    }

    /// Resolve the stable Agent identity that owns one live DSH Session id.
    pub(crate) async fn binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RuntimeBinding>> {
        self.connection
            .query_row::<RuntimeBinding>(
                &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings WHERE session_id = ?1"),
                [session_id],
            )
            .await
    }

    /// List every durable current runtime binding for process recovery.
    pub(crate) async fn all_bindings(&self) -> Result<Vec<RuntimeBinding>> {
        self.connection
            .query_rows::<RuntimeBinding>(
                &format!("SELECT {BINDING_COLUMNS} FROM runtime_bindings ORDER BY agent_id"),
                (),
            )
            .await
    }

    /// The generation counter for one Agent; zero before the first bind.
    pub(crate) async fn current_generation(&self, agent_id: &str) -> Result<i64> {
        let mut rows = self
            .connection
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
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        let mut rows = self
            .connection
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

    /// Insert or replace the durable current binding for this Agent.
    pub(crate) async fn upsert(&self, binding: &RuntimeBinding) -> Result<()> {
        self.connection
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
                    binding.agent_id.as_str(),
                    binding.session_id.as_str(),
                    binding.generation,
                    binding.provider.as_str(),
                    binding.model.as_str(),
                    binding.preset.as_str(),
                    binding.bound_at_ms,
                ),
            )
            .await?;
        Ok(())
    }

    /// Rewrite the preset label for exactly this Agent, Session, and
    /// generation.
    pub(crate) async fn update_preset(&self, binding: &RuntimeBinding, preset: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE runtime_bindings SET preset = ?1
                 WHERE agent_id = ?2 AND generation = ?3 AND session_id = ?4",
                (
                    preset,
                    binding.agent_id.as_str(),
                    binding.generation,
                    binding.session_id.as_str(),
                ),
            )
            .await?;
        Ok(())
    }
}
