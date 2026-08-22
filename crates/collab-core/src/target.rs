//! Target identity, authorization routes, and Channel/Direct creation.

use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::db::{ExecuteOne, FromRow, QueryRows};

use crate::actor::{Actor, ActorId};
use crate::changefeed::ChangeStore;
use crate::membership::{Membership, MembershipRole, MembershipStore};
use crate::{ChangeKind, CollabCore, CollabError, NonBlank, Result, new_id, now_ms};

/// A collab target kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Channel,
    Direct,
    Thread,
}

impl TargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Direct => "direct",
            Self::Thread => "thread",
        }
    }

    /// Decode one row's kind text, rejecting unknown values.
    pub(crate) fn parse(target_id: &str, value: &str) -> Result<Self> {
        match value {
            "channel" => Ok(Self::Channel),
            "direct" => Ok(Self::Direct),
            "thread" => Ok(Self::Thread),
            other => Err(CollabError::Database(format!(
                "target '{target_id}' has unknown kind '{other}'"
            ))),
        }
    }
}

/// A stable exact collab target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Target {
    pub id: String,
    pub kind: TargetKind,
    pub name: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: i64,
}

/// One active target's authorization route: kind proof plus the Thread
/// inheritance parent when present.
#[derive(Clone, Debug)]
pub(crate) struct TargetRoute {
    pub(crate) kind: TargetKind,
    pub(crate) parent_target_id: Option<String>,
}

impl TargetRoute {
    /// Load one active target's authorization route, failing when absent.
    /// Existence and topology proof: a Thread must carry a non-Thread parent.
    pub(crate) async fn require(connection: &Connection, target_id: &str) -> Result<Self> {
        let RouteRow {
            kind_text,
            parent_target_id,
        } = connection
            .query_row::<RouteRow>(
                "SELECT kind, parent_target_id
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?
            .ok_or_else(|| CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            })?;
        let kind = TargetKind::parse(target_id, &kind_text)?;
        if kind == TargetKind::Thread && parent_target_id.is_none() {
            return Err(CollabError::Database(format!(
                "Thread target '{target_id}' has no parent target"
            )));
        }
        if kind != TargetKind::Thread && parent_target_id.is_some() {
            return Err(CollabError::Database(format!(
                "non-Thread target '{target_id}' unexpectedly has a parent"
            )));
        }
        if let Some(parent_target_id) = parent_target_id.as_deref() {
            let parent_kind_text = connection
                .query_row::<String>(
                    "SELECT kind FROM targets
                     WHERE id = ?1 AND archived_at_ms IS NULL",
                    [parent_target_id],
                )
                .await?
                .ok_or_else(|| CollabError::NotFound {
                    entity: "active Thread parent target",
                    id: parent_target_id.to_owned(),
                })?;
            let parent_kind = TargetKind::parse(parent_target_id, &parent_kind_text)?;
            if parent_kind == TargetKind::Thread {
                return Err(CollabError::Database(format!(
                    "Thread target '{target_id}' has a Thread parent"
                )));
            }
        }
        Ok(Self {
            kind,
            parent_target_id,
        })
    }

    pub(crate) fn permission_target_id<'a>(&'a self, exact_target_id: &'a str) -> &'a str {
        self.parent_target_id.as_deref().unwrap_or(exact_target_id)
    }
}

/// One access's complete evidence: the loaded actor plus its proven route and
/// role. Obtained exclusively through [`AccessGrant::require`].
#[derive(Clone, Debug)]
pub(crate) struct AccessGrant {
    pub actor: Actor,
    pub route: TargetRoute,
    pub role: MembershipRole,
}

impl AccessGrant {
    /// Certify that `actor_id` exists and is an active member of
    /// `target_id`'s permission target, carrying every fact downstream
    /// decisions need (identity, topology, role) in one value.
    pub(crate) async fn require(
        connection: &Connection,
        target_id: &str,
        actor_id: &str,
    ) -> Result<Self> {
        let route = TargetRoute::require(connection, target_id).await?;
        let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
        let membership =
            Membership::require(connection, route.permission_target_id(target_id), &actor).await?;
        Ok(Self {
            actor,
            route,
            role: membership.role(),
        })
    }
}

impl CollabCore {
    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(&self, name: &str, creator_id: &str) -> Result<Target> {
        NonBlank::parse("channel name", name)?;
        NonBlank::parse("creator_id", creator_id)?;
        let creator_id = ActorId::parse(creator_id)?;
        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.to_owned(),
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.as_str().to_owned(),
            created_at_ms: now,
        };
        self.write(async |connection| {
            Actor::require(connection, &creator_id).await?;
            TargetStore::new(connection).insert(&target).await?;
            MembershipStore::new(connection)
                .insert(&target.id, &target.created_by, "owner", now)
                .await?;
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TargetCreated, &target.id, &target.id, &[], now)
                .await?;
            Ok(target)
        })
        .await
    }

    /// Return the one stable Direct target for an unordered pair of actors,
    /// creating it and its two memberships when absent.
    pub async fn create_direct(&self, actor_id: &str, peer_id: &str) -> Result<Target> {
        NonBlank::parse("actor_id", actor_id)?;
        NonBlank::parse("peer_id", peer_id)?;
        if actor_id == peer_id {
            return Err(CollabError::InvalidArgument(
                "a Direct target requires two distinct actors".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let peer = Actor::require(connection, &ActorId::parse(peer_id)?).await?;
            let (low, high) = if actor.id < peer.id {
                (&actor, &peer)
            } else {
                (&peer, &actor)
            };

            let existing_pair = connection
                .query_row::<String>(
                    "SELECT target_id FROM direct_pairs
                     WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                    (low.id.as_str(), high.id.as_str()),
                )
                .await?;
            if let Some(target_id) = existing_pair {
                return TargetStore::new(connection).find(&target_id).await;
            }

            let target = Target {
                id: new_id(),
                kind: TargetKind::Direct,
                name: format!("@{} ↔ @{}", low.handle, high.handle),
                parent_target_id: None,
                root_message_id: None,
                created_by: actor.id.clone(),
                created_at_ms: now,
            };
            TargetStore::new(connection).insert(&target).await?;
            for (member_id, role) in [(&actor.id, "owner"), (&peer.id, "member")] {
                MembershipStore::new(connection)
                    .insert(&target.id, member_id, role, now)
                    .await?;
            }
            TargetStore::new(connection)
                .insert_direct_pair(&target.id, low.id.as_str(), high.id.as_str(), now)
                .await?;
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TargetCreated, &target.id, &target.id, &[], now)
                .await?;
            Ok(target)
        })
        .await
    }
}

/// Persistence for the targets table; the only owner of its SQL.
pub(crate) struct TargetStore<'connection> {
    connection: &'connection Connection,
}

/// Column projection for one Target row.
struct TargetRow {
    id: String,
    kind_text: String,
    name: String,
    parent_target_id: Option<String>,
    root_message_id: Option<String>,
    created_by: String,
    created_at_ms: i64,
}

impl FromRow for TargetRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind_text: row.get(1)?,
            name: row.get(2)?,
            parent_target_id: row.get(3)?,
            root_message_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at_ms: row.get(6)?,
        })
    }
}

impl TargetRow {
    fn into_target(self) -> Result<Target> {
        let Self {
            id,
            kind_text,
            name,
            parent_target_id,
            root_message_id,
            created_by,
            created_at_ms,
        } = self;
        Ok(Target {
            kind: TargetKind::parse(&id, &kind_text)?,
            id,
            name,
            parent_target_id,
            root_message_id,
            created_by,
            created_at_ms,
        })
    }
}

/// Column projection for a Target authorization route.
struct RouteRow {
    kind_text: String,
    parent_target_id: Option<String>,
}

impl FromRow for RouteRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            kind_text: row.get(0)?,
            parent_target_id: row.get(1)?,
        })
    }
}

impl<'connection> TargetStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Insert one freshly built Target row.
    pub(crate) async fn insert(&self, target: &Target) -> Result<()> {
        self.connection
            .execute_one(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
                (
                    target.id.as_str(),
                    target.kind.as_str(),
                    target.name.as_str(),
                    target.parent_target_id.as_deref(),
                    target.root_message_id.as_deref(),
                    target.created_by.as_str(),
                    target.created_at_ms,
                ),
                "target insert",
            )
            .await
    }

    /// Record the unordered Actor pair that owns one Direct target.
    pub(crate) async fn insert_direct_pair(
        &self,
        target_id: &str,
        low_actor_id: &str,
        high_actor_id: &str,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute_one(
                "INSERT INTO direct_pairs
                 (target_id, actor_low_id, actor_high_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                (target_id, low_actor_id, high_actor_id, now),
                "direct pair insert",
            )
            .await
    }

    pub(crate) async fn find(&self, target_id: &str) -> Result<Target> {
        self.connection
            .query_row::<TargetRow>(
                "SELECT id, kind, name, parent_target_id, root_message_id,
                        created_by, created_at_ms
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?
            .map(|row| row.into_target())
            .transpose()?
            .ok_or_else(|| CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            })
    }

    pub(crate) async fn for_actor(&self, actor_id: &str) -> Result<Vec<Target>> {
        let rows = self
            .connection
            .query_rows::<TargetRow>(
                "SELECT DISTINCT target.id, target.kind, target.name,
                        target.parent_target_id, target.root_message_id,
                        target.created_by, target.created_at_ms
                 FROM v_target_access access
                 JOIN targets target ON target.id = access.target_id
                 WHERE access.actor_id = ?1
                   AND (
                     target.kind <> 'thread'
                     OR EXISTS (
                       SELECT 1 FROM targets parent
                       WHERE parent.id = target.parent_target_id
                         AND parent.kind IN ('channel', 'direct')
                         AND parent.archived_at_ms IS NULL
                     )
                   )
                 ORDER BY target.created_at_ms, target.id",
                [actor_id],
            )
            .await?;
        rows.into_iter().map(TargetRow::into_target).collect()
    }
}
