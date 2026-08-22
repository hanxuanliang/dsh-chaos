//! Stable actor identity and kind, with persistence anchored on the domain type.

use turso::{Connection, Row};

use crate::changefeed::{all_actor_ids, insert_change};
use crate::db::{FromRow, QueryRows};
use crate::{ChangeKind, CollabError, Result, new_id};

// ── 类型 ─────────────────────────────────────────────────────────────────────

/// Stable actor identifier, non-blank by construction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ActorId(String);

impl ActorId {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.trim().is_empty() {
            return Err(CollabError::InvalidArgument("actor_id must not be blank".into()));
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable collab actor kind.
#[derive(Clone, Copy, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
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
#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: i64,
}

// ── 证据 ─────────────────────────────────────────────────────────────────────

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

// ── 能力 ─────────────────────────────────────────────────────────────────────

/// Insert one Actor of `kind` and emit ActorCreated to every known actor.
pub(crate) async fn insert_actor(
    connection: &Connection,
    kind: ActorKind,
    handle: &str,
    display_name: &str,
    now: i64,
) -> Result<Actor> {
    require_non_blank!(handle, display_name);
    let actor = Actor {
        id: new_id(),
        kind,
        handle: handle.to_owned(),
        display_name: display_name.to_owned(),
        created_at_ms: now,
    };
    actor.insert(connection).await?;
    let actor_ids = all_actor_ids(connection).await?;
    insert_change(
        connection,
        ChangeKind::ActorCreated,
        None,
        &actor.id,
        &actor_ids,
        now,
    )
    .await?;
    Ok(actor)
}

// ── 存储 ─────────────────────────────────────────────────────────────────────

impl Actor {
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
}
