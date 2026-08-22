use turso::{Connection, Row};

use crate::actor::parse_actor_kind;
use crate::db::{FromRow, QueryRows};
use crate::{Actor, ActorKind, AgentProfile, CollabError, Result};

use super::model::{decode_charter, parse_agent_lifecycle};

/// Column order of the canonical Agent Profile projection: the Actor columns
/// followed by the Agent columns of the JOIN.
const PROFILE_COLUMNS: &str =
    "actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms,
     agent.workspace_path, agent.lifecycle, agent.charter_json,
     agent.profile_version, agent.created_at_ms, agent.updated_at_ms";

struct ProfileRow {
    id: String,
    kind: String,
    handle: String,
    display_name: String,
    actor_created_at_ms: i64,
    workspace_path: String,
    lifecycle: String,
    charter_json: String,
    version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl FromRow for ProfileRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            handle: row.get(2)?,
            display_name: row.get(3)?,
            actor_created_at_ms: row.get(4)?,
            workspace_path: row.get(5)?,
            lifecycle: row.get(6)?,
            charter_json: row.get(7)?,
            version: row.get(8)?,
            created_at_ms: row.get(9)?,
            updated_at_ms: row.get(10)?,
        })
    }
}

impl ProfileRow {
    fn into_profile(self) -> Result<AgentProfile> {
        let kind = parse_actor_kind(&self.id, &self.kind)?;
        if kind != ActorKind::Agent {
            return Err(CollabError::Database(format!(
                "Agent Profile '{}' belongs to a non-Agent actor",
                self.id
            )));
        }
        let lifecycle = parse_agent_lifecycle(&self.id, &self.lifecycle)?;
        let charter = decode_charter(&self.id, &self.charter_json)?;
        Ok(AgentProfile {
            actor: Actor {
                id: self.id,
                kind,
                handle: self.handle,
                display_name: self.display_name,
                created_at_ms: self.actor_created_at_ms,
            },
            workspace_path: self.workspace_path,
            lifecycle,
            charter,
            version: self.version,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

pub(crate) struct ProfileStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ProfileStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Load one Agent Profile, failing when the Agent is absent.
    pub(crate) async fn require_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        self.connection
            .query_row::<ProfileRow>(
                &format!(
                    "SELECT {PROFILE_COLUMNS}
                     FROM agents agent
                     JOIN actors actor ON actor.id = agent.actor_id
                     WHERE agent.actor_id = ?1"
                ),
                [agent_id],
            )
            .await?
            .map(ProfileRow::into_profile)
            .transpose()?
            .ok_or_else(|| CollabError::NotFound {
                entity: "agent profile",
                id: agent_id.to_owned(),
            })
    }

    /// List every live Agent Profile, handle-ordered.
    pub(crate) async fn directory(&self) -> Result<Vec<AgentProfile>> {
        self.connection
            .query_rows::<ProfileRow>(
                &format!(
                    "SELECT {PROFILE_COLUMNS}
                     FROM agents agent
                     JOIN actors actor ON actor.id = agent.actor_id
                     ORDER BY actor.handle, actor.id"
                ),
                (),
            )
            .await?
            .into_iter()
            .map(ProfileRow::into_profile)
            .collect()
    }

    pub(crate) async fn update_charter(
        &self,
        agent_id: &str,
        charter_json: &str,
        next_version: i64,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE agents
                 SET charter_json = ?2, profile_version = ?3, updated_at_ms = ?4
                 WHERE actor_id = ?1",
                (agent_id, charter_json, next_version, now),
            )
            .await?;
        Ok(())
    }

    /// Insert the agents row of one freshly created Agent actor.
    pub(crate) async fn insert_agent(
        &self,
        actor_id: &str,
        workspace_path: &str,
        charter_json: &str,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO agents
                 (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms,
                  charter_json, profile_version)
                 VALUES (?1, ?2, 'active', ?3, ?3, ?4, 1)",
                (actor_id, workspace_path, now, charter_json),
            )
            .await?;
        Ok(())
    }

    /// Delete the Agent's operational state; its actors row and Messages stay
    /// so history never points at a missing author.
    pub(crate) async fn delete_operational_state(&self, agent_id: &str) -> Result<()> {
        for (sql, params) in [
            ("DELETE FROM memberships WHERE actor_id = ?1", [agent_id]),
            (
                "DELETE FROM runtime_bindings WHERE agent_id = ?1",
                [agent_id],
            ),
            ("DELETE FROM agents WHERE actor_id = ?1", [agent_id]),
            (
                "DELETE FROM agent_wake_state WHERE agent_id = ?1",
                [agent_id],
            ),
            (
                "DELETE FROM inbox_batch_items
                 WHERE batch_id IN (SELECT id FROM inbox_batches WHERE agent_id = ?1)",
                [agent_id],
            ),
            ("DELETE FROM inbox_batches WHERE agent_id = ?1", [agent_id]),
        ] {
            self.connection.execute(sql, params).await?;
        }
        Ok(())
    }
}
