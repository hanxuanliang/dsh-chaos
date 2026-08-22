use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::db::{FromRow, QueryRows};
use crate::{CollabError, Result};

string_id!(ActorId, "actor_id");

/// A stable collab actor kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
}

impl ActorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

/// A stable collab actor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: i64,
}

impl FromRow for Actor {
    fn from_row(row: &Row) -> Result<Self> {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        Ok(Self {
            kind: parse_actor_kind(&id, &kind_text)?,
            id,
            handle: row.get(2)?,
            display_name: row.get(3)?,
            created_at_ms: row.get(4)?,
        })
    }
}

impl Actor {
    /// Load one Actor by id, failing when absent. Presence proof for
    /// authorization chains.
    pub(crate) async fn require(connection: &Connection, id: &ActorId) -> Result<Self> {
        connection
            .query_row::<Self>(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors WHERE id = ?1",
                [id.as_str()],
            )
            .await?
            .ok_or_else(|| CollabError::NotFound {
                entity: "actor",
                id: id.as_str().to_owned(),
            })
    }

    /// Load one Actor by handle when present.
    pub(crate) async fn find_by_handle(
        connection: &Connection,
        handle: &str,
    ) -> Result<Option<Self>> {
        connection
            .query_row::<Self>(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors WHERE handle = ?1",
                [handle],
            )
            .await
    }

    pub(crate) async fn insert(&self, connection: &Connection) -> Result<()> {
        connection
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    self.id.as_str(),
                    self.kind.as_str(),
                    self.handle.as_str(),
                    self.display_name.as_str(),
                    self.created_at_ms,
                ),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn rename(
        connection: &Connection,
        actor_id: &str,
        display_name: &str,
    ) -> Result<()> {
        connection
            .execute(
                "UPDATE actors SET display_name = ?2 WHERE id = ?1",
                (actor_id, display_name),
            )
            .await?;
        Ok(())
    }

    /// Load one Actor, failing unless it is an active Agent.
    pub(crate) async fn require_agent(connection: &Connection, id: &ActorId) -> Result<Self> {
        let actor = Self::require(connection, id).await?;
        if actor.kind != ActorKind::Agent {
            return Err(CollabError::NotFound {
                entity: "agent",
                id: id.as_str().to_owned(),
            });
        }
        let mut rows = connection
            .query(
                "SELECT 1 FROM agents WHERE actor_id = ?1 AND lifecycle = 'active'",
                [id.as_str()],
            )
            .await?;
        if rows.next().await?.is_none() {
            return Err(CollabError::NotFound {
                entity: "active agent",
                id: id.as_str().to_owned(),
            });
        }
        Ok(actor)
    }
}

pub(crate) fn parse_actor_kind(actor_id: &str, value: &str) -> Result<ActorKind> {
    match value {
        "user" => Ok(ActorKind::User),
        "agent" => Ok(ActorKind::Agent),
        other => Err(CollabError::Database(format!(
            "actor '{actor_id}' has unknown kind '{other}'"
        ))),
    }
}
