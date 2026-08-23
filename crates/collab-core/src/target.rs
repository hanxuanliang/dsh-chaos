//! Target identity, authorization routes, and Channel/Direct creation.

use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::db::{ExecuteOne, FromRow, QueryRows};

use crate::actor::{Actor, ActorId};
use crate::changefeed::ChangeStore;
use crate::membership::{Membership, MembershipRole, MembershipStore};
use crate::thread::store::ThreadStore;
use crate::{ChangeKind, CollabCore, CollabError, NonBlank, Result, new_id, now_ms};

/// A collab target kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Channel,
    Direct,
    Thread,
}

/// Operational lifecycle of a Channel and its inherited Thread targets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetLifecycle {
    Active,
    Archived,
    Deleted,
}

impl TargetLifecycle {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
        }
    }

    fn from_timestamps(archived_at_ms: Option<i64>, deleted_at_ms: Option<i64>) -> Self {
        if deleted_at_ms.is_some() {
            Self::Deleted
        } else if archived_at_ms.is_some() {
            Self::Archived
        } else {
            Self::Active
        }
    }
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
    pub description: String,
    pub lifecycle: TargetLifecycle,
    pub version: i64,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived_at_ms: Option<i64>,
    pub deleted_at_ms: Option<i64>,
}

/// One active target's authorization route: kind proof plus the Thread
/// inheritance parent when present.
#[derive(Clone, Debug)]
pub(crate) struct TargetRoute {
    pub(crate) kind: TargetKind,
    pub(crate) parent_target_id: Option<String>,
    pub(crate) lifecycle: TargetLifecycle,
}

impl TargetRoute {
    /// Load one target's authorization route, including archived/deleted
    /// tombstones for exact historical reads.
    /// Existence and topology proof: a Thread must carry a non-Thread parent.
    pub(crate) async fn require(connection: &Connection, target_id: &str) -> Result<Self> {
        let RouteRow {
            kind_text,
            parent_target_id,
            archived_at_ms,
            deleted_at_ms,
        } = connection
            .query_row::<RouteRow>(
                "SELECT kind, parent_target_id, archived_at_ms, deleted_at_ms
                 FROM targets WHERE id = ?1",
                [target_id],
            )
            .await?
            .ok_or_else(|| CollabError::NotFound {
                entity: "target",
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
        let mut lifecycle = TargetLifecycle::from_timestamps(archived_at_ms, deleted_at_ms);
        if let Some(parent_target_id) = parent_target_id.as_deref() {
            let parent = connection
                .query_row::<RouteRow>(
                    "SELECT kind, parent_target_id, archived_at_ms, deleted_at_ms
                     FROM targets WHERE id = ?1",
                    [parent_target_id],
                )
                .await?
                .ok_or_else(|| CollabError::NotFound {
                    entity: "Thread parent target",
                    id: parent_target_id.to_owned(),
                })?;
            let parent_kind = TargetKind::parse(parent_target_id, &parent.kind_text)?;
            if parent_kind == TargetKind::Thread {
                return Err(CollabError::Database(format!(
                    "Thread target '{target_id}' has a Thread parent"
                )));
            }
            let parent_lifecycle =
                TargetLifecycle::from_timestamps(parent.archived_at_ms, parent.deleted_at_ms);
            if parent_lifecycle != TargetLifecycle::Active {
                lifecycle = parent_lifecycle;
            }
        }
        Ok(Self {
            kind,
            parent_target_id,
            lifecycle,
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
        let membership = if route.lifecycle == TargetLifecycle::Deleted {
            Membership::require_historical(
                connection,
                route.permission_target_id(target_id),
                &actor,
            )
            .await?
        } else {
            Membership::require(connection, route.permission_target_id(target_id), &actor).await?
        };
        Ok(Self {
            actor,
            route,
            role: membership.role(),
        })
    }

    /// Certify that an actor may mutate an active target.
    pub(crate) async fn require_writable(
        connection: &Connection,
        target_id: &str,
        actor_id: &str,
    ) -> Result<Self> {
        let grant = Self::require(connection, target_id, actor_id).await?;
        if grant.route.lifecycle != TargetLifecycle::Active {
            return Err(CollabError::TargetNotWritable {
                target_id: target_id.to_owned(),
                lifecycle: grant.route.lifecycle.as_str().to_owned(),
            });
        }
        Ok(grant)
    }

    pub(crate) fn require_owner(&self, target_id: &str) -> Result<()> {
        if self.role != MembershipRole::Owner {
            return Err(CollabError::PermissionDenied {
                actor_id: self.actor.id.clone(),
                action: "manage",
                target_id: target_id.to_owned(),
            });
        }
        Ok(())
    }
}

impl CollabCore {
    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(
        &self,
        name: &str,
        description: &str,
        creator_id: &str,
    ) -> Result<Target> {
        NonBlank::parse("channel name", name)?;
        NonBlank::parse("channel description", description)?;
        if name.chars().count() > 64 {
            return Err(CollabError::InvalidArgument(
                "channel name must not exceed 64 characters".into(),
            ));
        }
        if description.chars().count() > 280 {
            return Err(CollabError::InvalidArgument(
                "channel description must not exceed 280 characters".into(),
            ));
        }
        NonBlank::parse("creator_id", creator_id)?;
        let creator_id = ActorId::parse(creator_id)?;
        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.trim().to_owned(),
            description: description.trim().to_owned(),
            lifecycle: TargetLifecycle::Active,
            version: 1,
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.as_str().to_owned(),
            created_at_ms: now,
            updated_at_ms: now,
            archived_at_ms: None,
            deleted_at_ms: None,
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
                description: String::new(),
                lifecycle: TargetLifecycle::Active,
                version: 1,
                parent_target_id: None,
                root_message_id: None,
                created_by: actor.id.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                archived_at_ms: None,
                deleted_at_ms: None,
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

    /// Rename a writable Channel and replace its concise collaboration context.
    pub async fn update_channel(
        &self,
        target_id: &str,
        actor_id: &str,
        name: &str,
        description: &str,
        expected_version: i64,
    ) -> Result<Target> {
        validate_channel_details(name, description, expected_version)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let grant = AccessGrant::require_writable(connection, target_id, actor_id).await?;
            grant.require_owner(target_id)?;
            let store = TargetStore::new(connection);
            let current = store.require_channel(target_id).await?;
            require_target_version(&current, expected_version)?;
            if !store
                .update_channel_details(
                    target_id,
                    name.trim(),
                    description.trim(),
                    expected_version,
                    now,
                )
                .await?
            {
                return Err(target_version_conflict(
                    target_id,
                    expected_version,
                    store.require_channel(target_id).await?.version,
                ));
            }
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TargetChanged, target_id, target_id, &[], now)
                .await?;
            store.require_channel(target_id).await
        })
        .await
    }

    /// Make one active Channel and every child Thread read-only.
    pub async fn archive_channel(
        &self,
        target_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Target> {
        require_positive_version(expected_version)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let grant = AccessGrant::require_writable(connection, target_id, actor_id).await?;
            grant.require_owner(target_id)?;
            let store = TargetStore::new(connection);
            let current = store.require_channel(target_id).await?;
            require_target_version(&current, expected_version)?;
            if !store
                .archive_channel(target_id, expected_version, now)
                .await?
            {
                return Err(target_version_conflict(
                    target_id,
                    expected_version,
                    store.require_channel(target_id).await?.version,
                ));
            }
            store.archive_child_threads(target_id, now).await?;
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TargetChanged, target_id, target_id, &[], now)
                .await?;
            store.require_channel(target_id).await
        })
        .await
    }

    /// Restore one archived Channel and its child Threads to writable state.
    pub async fn restore_channel(
        &self,
        target_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Target> {
        require_positive_version(expected_version)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let grant = AccessGrant::require(connection, target_id, actor_id).await?;
            grant.require_owner(target_id)?;
            if grant.route.lifecycle != TargetLifecycle::Archived {
                return Err(CollabError::TargetNotWritable {
                    target_id: target_id.to_owned(),
                    lifecycle: grant.route.lifecycle.as_str().to_owned(),
                });
            }
            let store = TargetStore::new(connection);
            let current = store.require_channel(target_id).await?;
            require_target_version(&current, expected_version)?;
            if !store
                .restore_channel(target_id, expected_version, now)
                .await?
            {
                return Err(target_version_conflict(
                    target_id,
                    expected_version,
                    store.require_channel(target_id).await?.version,
                ));
            }
            store.restore_child_threads(target_id, now).await?;
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TargetChanged, target_id, target_id, &[], now)
                .await?;
            store.require_channel(target_id).await
        })
        .await
    }

    /// Soft-delete one Channel while retaining its immutable collaboration history.
    pub async fn delete_channel(
        &self,
        target_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Target> {
        require_positive_version(expected_version)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let grant = AccessGrant::require(connection, target_id, actor_id).await?;
            grant.require_owner(target_id)?;
            if grant.route.lifecycle == TargetLifecycle::Deleted {
                return Err(CollabError::TargetNotWritable {
                    target_id: target_id.to_owned(),
                    lifecycle: TargetLifecycle::Deleted.as_str().to_owned(),
                });
            }
            let store = TargetStore::new(connection);
            let current = store.require_channel(target_id).await?;
            require_target_version(&current, expected_version)?;
            let recipients = MembershipStore::new(connection)
                .active_member_ids(target_id)
                .await?;
            if !store
                .delete_channel(target_id, expected_version, now)
                .await?
            {
                return Err(target_version_conflict(
                    target_id,
                    expected_version,
                    store.require_channel(target_id).await?.version,
                ));
            }
            store.delete_child_threads(target_id, now).await?;
            ChangeStore::new(connection)
                .insert_change(
                    ChangeKind::TargetChanged,
                    Some(target_id),
                    target_id,
                    &recipients,
                    now,
                )
                .await?;
            MembershipStore::new(connection)
                .end_all_active(target_id, now)
                .await?;
            ThreadStore::new(connection)
                .end_follows_for_parent(target_id, now)
                .await?;
            store.require_channel(target_id).await
        })
        .await
    }
}

fn validate_channel_details(name: &str, description: &str, expected_version: i64) -> Result<()> {
    NonBlank::parse("channel name", name)?;
    NonBlank::parse("channel description", description)?;
    if name.chars().count() > 64 {
        return Err(CollabError::InvalidArgument(
            "channel name must not exceed 64 characters".into(),
        ));
    }
    if description.chars().count() > 280 {
        return Err(CollabError::InvalidArgument(
            "channel description must not exceed 280 characters".into(),
        ));
    }
    require_positive_version(expected_version)
}

fn require_positive_version(expected_version: i64) -> Result<()> {
    if expected_version < 1 {
        return Err(CollabError::InvalidArgument(
            "expected_version must be positive".into(),
        ));
    }
    Ok(())
}

fn require_target_version(target: &Target, expected_version: i64) -> Result<()> {
    if target.version != expected_version {
        return Err(target_version_conflict(
            &target.id,
            expected_version,
            target.version,
        ));
    }
    Ok(())
}

fn target_version_conflict(target_id: &str, expected: i64, actual: i64) -> CollabError {
    CollabError::TargetVersionConflict {
        target_id: target_id.to_owned(),
        expected,
        actual,
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
    description: String,
    version: i64,
    parent_target_id: Option<String>,
    root_message_id: Option<String>,
    created_by: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    archived_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
}

impl FromRow for TargetRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind_text: row.get(1)?,
            name: row.get(2)?,
            description: row.get(3)?,
            version: row.get(4)?,
            parent_target_id: row.get(5)?,
            root_message_id: row.get(6)?,
            created_by: row.get(7)?,
            created_at_ms: row.get(8)?,
            updated_at_ms: row.get(9)?,
            archived_at_ms: row.get(10)?,
            deleted_at_ms: row.get(11)?,
        })
    }
}

impl TargetRow {
    fn into_target(self) -> Result<Target> {
        let Self {
            id,
            kind_text,
            name,
            description,
            version,
            parent_target_id,
            root_message_id,
            created_by,
            created_at_ms,
            updated_at_ms,
            archived_at_ms,
            deleted_at_ms,
        } = self;
        Ok(Target {
            kind: TargetKind::parse(&id, &kind_text)?,
            id,
            name,
            description,
            lifecycle: TargetLifecycle::from_timestamps(archived_at_ms, deleted_at_ms),
            version,
            parent_target_id,
            root_message_id,
            created_by,
            created_at_ms,
            updated_at_ms,
            archived_at_ms,
            deleted_at_ms,
        })
    }
}

/// Column projection for a Target authorization route.
struct RouteRow {
    kind_text: String,
    parent_target_id: Option<String>,
    archived_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
}

impl FromRow for RouteRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            kind_text: row.get(0)?,
            parent_target_id: row.get(1)?,
            archived_at_ms: row.get(2)?,
            deleted_at_ms: row.get(3)?,
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
                 (id, kind, name, description, version, parent_target_id, root_message_id,
                  created_by, created_at_ms, updated_at_ms, archived_at_ms, deleted_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                (
                    target.id.as_str(),
                    target.kind.as_str(),
                    target.name.as_str(),
                    target.description.as_str(),
                    target.version,
                    target.parent_target_id.as_deref(),
                    target.root_message_id.as_deref(),
                    target.created_by.as_str(),
                    target.created_at_ms,
                    target.updated_at_ms,
                    target.archived_at_ms,
                    target.deleted_at_ms,
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
                "SELECT id, kind, name, description, version, parent_target_id, root_message_id,
                        created_by, created_at_ms, updated_at_ms, archived_at_ms, deleted_at_ms
                 FROM targets WHERE id = ?1",
                [target_id],
            )
            .await?
            .map(|row| row.into_target())
            .transpose()?
            .ok_or_else(|| CollabError::NotFound {
                entity: "target",
                id: target_id.to_owned(),
            })
    }

    pub(crate) async fn require_channel(&self, target_id: &str) -> Result<Target> {
        let target = self.find(target_id).await?;
        if target.kind != TargetKind::Channel {
            return Err(CollabError::InvalidArgument(
                "Channel lifecycle operations only support Channel targets".into(),
            ));
        }
        Ok(target)
    }

    pub(crate) async fn update_channel_details(
        &self,
        target_id: &str,
        name: &str,
        description: &str,
        expected_version: i64,
        now: i64,
    ) -> Result<bool> {
        Ok(self
            .connection
            .execute(
                "UPDATE targets
                 SET name = ?2, description = ?3, version = version + 1, updated_at_ms = ?4
                 WHERE id = ?1 AND kind = 'channel' AND version = ?5
                   AND archived_at_ms IS NULL AND deleted_at_ms IS NULL",
                (target_id, name, description, now, expected_version),
            )
            .await?
            == 1)
    }

    pub(crate) async fn archive_channel(
        &self,
        target_id: &str,
        expected_version: i64,
        now: i64,
    ) -> Result<bool> {
        Ok(self
            .connection
            .execute(
                "UPDATE targets
                 SET archived_at_ms = ?3, version = version + 1, updated_at_ms = ?3
                 WHERE id = ?1 AND kind = 'channel' AND version = ?2
                   AND archived_at_ms IS NULL AND deleted_at_ms IS NULL",
                (target_id, expected_version, now),
            )
            .await?
            == 1)
    }

    pub(crate) async fn restore_channel(
        &self,
        target_id: &str,
        expected_version: i64,
        now: i64,
    ) -> Result<bool> {
        Ok(self
            .connection
            .execute(
                "UPDATE targets
                 SET archived_at_ms = NULL, version = version + 1, updated_at_ms = ?3
                 WHERE id = ?1 AND kind = 'channel' AND version = ?2
                   AND archived_at_ms IS NOT NULL AND deleted_at_ms IS NULL",
                (target_id, expected_version, now),
            )
            .await?
            == 1)
    }

    pub(crate) async fn delete_channel(
        &self,
        target_id: &str,
        expected_version: i64,
        now: i64,
    ) -> Result<bool> {
        Ok(self
            .connection
            .execute(
                "UPDATE targets
                 SET deleted_at_ms = ?3, version = version + 1, updated_at_ms = ?3
                 WHERE id = ?1 AND kind = 'channel' AND version = ?2
                   AND deleted_at_ms IS NULL",
                (target_id, expected_version, now),
            )
            .await?
            == 1)
    }

    pub(crate) async fn archive_child_threads(&self, target_id: &str, now: i64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE targets
                 SET archived_at_ms = ?2, version = version + 1, updated_at_ms = ?2
                 WHERE parent_target_id = ?1 AND kind = 'thread' AND deleted_at_ms IS NULL",
                (target_id, now),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn restore_child_threads(&self, target_id: &str, now: i64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE targets
                 SET archived_at_ms = NULL, version = version + 1, updated_at_ms = ?2
                 WHERE parent_target_id = ?1 AND kind = 'thread' AND deleted_at_ms IS NULL",
                (target_id, now),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn delete_child_threads(&self, target_id: &str, now: i64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE targets
                 SET deleted_at_ms = ?2, version = version + 1, updated_at_ms = ?2
                 WHERE parent_target_id = ?1 AND kind = 'thread' AND deleted_at_ms IS NULL",
                (target_id, now),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn for_actor(&self, actor_id: &str) -> Result<Vec<Target>> {
        let rows = self
            .connection
            .query_rows::<TargetRow>(
                "SELECT DISTINCT target.id, target.kind, target.name, target.description,
                        target.version, target.parent_target_id, target.root_message_id,
                        target.created_by, target.created_at_ms, target.updated_at_ms,
                        target.archived_at_ms, target.deleted_at_ms
                 FROM v_target_access access
                 JOIN targets target ON target.id = access.target_id
                 WHERE access.actor_id = ?1
                   AND (
                     target.kind <> 'thread'
                     OR EXISTS (
                       SELECT 1 FROM targets parent
                       WHERE parent.id = target.parent_target_id
                         AND parent.kind IN ('channel', 'direct')
                         AND parent.deleted_at_ms IS NULL
                     )
                   )
                 ORDER BY target.created_at_ms, target.id",
                [actor_id],
            )
            .await?;
        rows.into_iter().map(TargetRow::into_target).collect()
    }
}
