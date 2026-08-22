//! Target identity, authorization routes, and Channel/Direct creation.

use serde::{Deserialize, Serialize};
use turso::Connection;

use crate::actor::{Actor, ActorId};
use crate::changefeed::ChangeStore;
use crate::membership::{Membership, MembershipRole};
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
        let mut rows = connection
            .query(
                "SELECT kind, parent_target_id
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            });
        };
        let kind_text = row.get::<String>(0)?;
        let parent_target_id = row.get::<Option<String>>(1)?;
        drop(rows);
        let kind = parse_target_kind(target_id, &kind_text)?;
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
            let mut parent_rows = connection
                .query(
                    "SELECT kind FROM targets
                     WHERE id = ?1 AND archived_at_ms IS NULL",
                    [parent_target_id],
                )
                .await?;
            let Some(parent_row) = parent_rows.next().await? else {
                return Err(CollabError::NotFound {
                    entity: "active Thread parent target",
                    id: parent_target_id.to_owned(),
                });
            };
            let parent_kind_text = parent_row.get::<String>(0)?;
            let parent_kind = parse_target_kind(parent_target_id, &parent_kind_text)?;
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

pub(crate) fn parse_target_kind(target_id: &str, value: &str) -> Result<TargetKind> {
    match value {
        "channel" => Ok(TargetKind::Channel),
        "direct" => Ok(TargetKind::Direct),
        "thread" => Ok(TargetKind::Thread),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' has unknown kind '{other}'"
        ))),
    }
}

pub(crate) async fn require_target(connection: &Connection, target_id: &str) -> Result<TargetKind> {
    Ok(TargetRoute::require(connection, target_id).await?.kind)
}

/// Certify that `actor_id` exists and is an active member of `target_id`'s
/// permission target, returning the resolved route.
pub(crate) async fn require_target_access(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<TargetRoute> {
    let route = TargetRoute::require(connection, target_id).await?;
    let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
    Membership::require(connection, route.permission_target_id(target_id), &actor).await?;
    Ok(route)
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
            connection
                .execute(
                    "INSERT INTO targets
                     (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL)",
                    (
                        target.id.as_str(),
                        target.kind.as_str(),
                        target.name.as_str(),
                        target.created_by.as_str(),
                        target.created_at_ms,
                    ),
                )
                .await?;
            connection
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, 'owner', ?3, NULL)",
                    (target.id.as_str(), target.created_by.as_str(), now),
                )
                .await?;
            ChangeStore::new(connection)
                .insert_target_change(
                ChangeKind::TargetCreated,
                &target.id,
                &target.id,
                &[],
                now,
            )
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

            let mut rows = connection
                .query(
                    "SELECT target_id FROM direct_pairs
                     WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                    (low.id.as_str(), high.id.as_str()),
                )
                .await?;
            if let Some(row) = rows.next().await? {
                let target_id = row.get::<String>(0)?;
                drop(rows);
                return TargetStore::new(connection).find(&target_id).await;
            }
            drop(rows);

            let target = Target {
                id: new_id(),
                kind: TargetKind::Direct,
                name: format!("@{} ↔ @{}", low.handle, high.handle),
                parent_target_id: None,
                root_message_id: None,
                created_by: actor.id.clone(),
                created_at_ms: now,
            };
            connection
                .execute(
                    "INSERT INTO targets
                     (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL)",
                    (
                        target.id.as_str(),
                        target.kind.as_str(),
                        target.name.as_str(),
                        target.created_by.as_str(),
                        now,
                    ),
                )
                .await?;
            for (member_id, role) in [(&actor.id, "owner"), (&peer.id, "member")] {
                connection
                    .execute(
                        "INSERT INTO memberships
                         (target_id, actor_id, role, joined_at_ms, left_at_ms)
                         VALUES (?1, ?2, ?3, ?4, NULL)",
                        (target.id.as_str(), member_id.as_str(), role, now),
                    )
                    .await?;
            }
            connection
                .execute(
                    "INSERT INTO direct_pairs
                     (target_id, actor_low_id, actor_high_id, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    (
                        target.id.as_str(),
                        low.id.as_str(),
                        high.id.as_str(),
                        now,
                    ),
                )
                .await?;
            ChangeStore::new(connection)
                .insert_target_change(
                ChangeKind::TargetCreated,
                &target.id,
                &target.id,
                &[],
                now,
            )
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

impl<'connection> TargetStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn find(&self, target_id: &str) -> Result<Target> {
        let mut rows = self
            .connection
            .query(
                "SELECT id, kind, name, parent_target_id, root_message_id,
                        created_by, created_at_ms
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            });
        };
        let kind_text = row.get::<String>(1)?;
        Ok(Target {
            id: row.get(0)?,
            kind: parse_target_kind(target_id, &kind_text)?,
            name: row.get(2)?,
            parent_target_id: row.get(3)?,
            root_message_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at_ms: row.get(6)?,
        })
    }

    pub(crate) async fn for_actor(&self, actor_id: &str) -> Result<Vec<Target>> {
        let mut rows = self
            .connection
            .query(
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
        let mut targets = Vec::new();
        while let Some(row) = rows.next().await? {
            let id = row.get::<String>(0)?;
            let kind_text = row.get::<String>(1)?;
            targets.push(Target {
                kind: parse_target_kind(&id, &kind_text)?,
                id,
                name: row.get(2)?,
                parent_target_id: row.get(3)?,
                root_message_id: row.get(4)?,
                created_by: row.get(5)?,
                created_at_ms: row.get(6)?,
            });
        }
        Ok(targets)
    }
}
