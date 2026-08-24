//! Stable actor identity and kind, with persistence anchored on the domain type.

use turso::{Connection, Row};

use crate::changefeed::ChangeStore;
use crate::db::{ExecuteOne, FromRow, QueryRows};
use crate::{ChangeKind, CollabError, NonBlank, Result, new_id};

/// Stable actor identifier, non-blank by construction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ActorId(String);

impl ActorId {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.trim().is_empty() {
            return Err(CollabError::InvalidArgument(
                "actor_id must not be blank".into(),
            ));
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

    /// Decode one row's kind text, rejecting unknown values.
    pub(crate) fn parse(actor_id: &str, value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Self::User),
            "agent" => Ok(Self::Agent),
            other => Err(CollabError::Database(format!(
                "actor '{actor_id}' has unknown kind '{other}'"
            ))),
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
    pub avatar_data_url: Option<String>,
}

impl FromRow for Actor {
    fn from_row(row: &Row) -> Result<Self> {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        Ok(Self {
            kind: ActorKind::parse(&id, &kind_text)?,
            id,
            handle: row.get(2)?,
            display_name: row.get(3)?,
            created_at_ms: row.get(4)?,
            avatar_data_url: row.get(5)?,
        })
    }
}

impl Actor {
    /// Load one Actor by id, failing when absent. Presence proof for
    /// authorization chains.
    pub(crate) async fn require(connection: &Connection, id: &ActorId) -> Result<Self> {
        ActorStore::new(connection)
            .find_by_id(id)
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
        ActorStore::new(connection).find_by_handle(handle).await
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
        if !ActorStore::new(connection).has_active_agent_row(id).await? {
            return Err(CollabError::NotFound {
                entity: "active agent",
                id: id.as_str().to_owned(),
            });
        }
        Ok(actor)
    }

    /// Insert one Actor of `kind` and emit ActorCreated to every known actor.
    /// Creation evidence: guards, persistence, and broadcast in one step.
    pub(crate) async fn insert(
        connection: &Connection,
        kind: ActorKind,
        handle: &str,
        display_name: &str,
        now: i64,
    ) -> Result<Self> {
        let handle = NonBlank::parse("handle", handle)?;
        let display_name = NonBlank::parse("display_name", display_name)?;
        let actor = Self {
            id: new_id(),
            kind,
            handle: handle.as_str().to_owned(),
            display_name: display_name.as_str().to_owned(),
            created_at_ms: now,
            avatar_data_url: None,
        };
        ActorStore::new(connection).insert(&actor).await?;
        let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
        ChangeStore::new(connection)
            .insert_change(ChangeKind::ActorCreated, None, &actor.id, &actor_ids, now)
            .await?;
        Ok(actor)
    }
}

/// Persistence for the actors table; the only owner of its SQL.
pub(crate) struct ActorStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ActorStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn find_by_id(&self, id: &ActorId) -> Result<Option<Actor>> {
        self.connection
            .query_row::<Actor>(
                "SELECT id, kind, handle, display_name, created_at_ms, avatar_data_url
                 FROM actors WHERE id = ?1",
                [id.as_str()],
            )
            .await
    }

    pub(crate) async fn find_by_handle(&self, handle: &str) -> Result<Option<Actor>> {
        self.connection
            .query_row::<Actor>(
                "SELECT id, kind, handle, display_name, created_at_ms, avatar_data_url
                 FROM actors WHERE handle = ?1",
                [handle],
            )
            .await
    }

    pub(crate) async fn has_active_agent_row(&self, id: &ActorId) -> Result<bool> {
        self.connection
            .exists(
                "SELECT 1 FROM agents WHERE actor_id = ?1 AND lifecycle = 'active'",
                [id.as_str()],
            )
            .await
    }

    pub(crate) async fn insert(&self, actor: &Actor) -> Result<()> {
        self.connection
            .execute_one(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    actor.id.as_str(),
                    actor.kind.as_str(),
                    actor.handle.as_str(),
                    actor.display_name.as_str(),
                    actor.created_at_ms,
                ),
                "actor insert",
            )
            .await
    }

    pub(crate) async fn rename(&self, id: &ActorId, display_name: &str) -> Result<()> {
        self.connection
            .execute_one(
                "UPDATE actors SET display_name = ?2 WHERE id = ?1",
                (id.as_str(), display_name),
                "actor rename",
            )
            .await
    }


    pub(crate) async fn update_avatar_data_url(
        &self,
        id: &ActorId,
        avatar_data_url: Option<&str>,
    ) -> Result<()> {
        self.connection
            .execute_one(
                "UPDATE actors SET avatar_data_url = ?2 WHERE id = ?1",
                (id.as_str(), avatar_data_url),
                "actor avatar update",
            )
            .await
    }
}
