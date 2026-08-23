//! Durable, recipient-filtered change notifications for client synchronization.

use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::actor::Actor;
use crate::actor::ActorId;
use crate::db::{FromRow, QueryRows, require_scalar_row};
use crate::membership::MembershipStore;
use crate::target::TargetStore;
use crate::task::store::TaskStore;
use crate::thread::store::ThreadStore;
use crate::{CollabCore, CollabError, Result, Target, Task};

/// A durable UI invalidation kind. Change rows carry identifiers rather than
/// mutable domain payloads; consumers re-read the authoritative projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    ActorCreated,
    AgentProfileChanged,
    TargetCreated,
    TargetChanged,
    MembershipChanged,
    ThreadFollowChanged,
    MessageCreated,
    TaskCreated,
    TaskUpdated,
    ActivityDoneChanged,
}

impl ChangeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ActorCreated => "actor_created",
            Self::AgentProfileChanged => "agent_profile_changed",
            Self::TargetCreated => "target_created",
            Self::TargetChanged => "target_changed",
            Self::MembershipChanged => "membership_changed",
            Self::ThreadFollowChanged => "thread_follow_changed",
            Self::MessageCreated => "message_created",
            Self::TaskCreated => "task_created",
            Self::TaskUpdated => "task_updated",
            Self::ActivityDoneChanged => "activity_done_changed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "actor_created" => Some(Self::ActorCreated),
            "agent_profile_changed" => Some(Self::AgentProfileChanged),
            "target_created" => Some(Self::TargetCreated),
            "target_changed" => Some(Self::TargetChanged),
            "membership_changed" => Some(Self::MembershipChanged),
            "thread_follow_changed" => Some(Self::ThreadFollowChanged),
            "message_created" => Some(Self::MessageCreated),
            "task_created" => Some(Self::TaskCreated),
            "task_updated" => Some(Self::TaskUpdated),
            "activity_done_changed" => Some(Self::ActivityDoneChanged),
            _ => None,
        }
    }
}

/// One monotonically ordered, recipient-snapshotted collaboration change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeEvent {
    pub seq: i64,
    pub kind: ChangeKind,
    pub target_id: Option<String>,
    pub entity_id: String,
    pub created_at_ms: i64,
}

/// One authorization-filtered bootstrap projection and its durable cursor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollabSnapshot {
    pub actor: Actor,
    pub cursor: i64,
    pub targets: Vec<Target>,
    pub followed_thread_ids: Vec<String>,
    pub tasks: Vec<Task>,
}

impl FromRow for ChangeEvent {
    fn from_row(row: &Row) -> Result<Self> {
        let seq = row.get::<i64>(0)?;
        let kind_text = row.get::<String>(1)?;
        let Some(kind) = ChangeKind::parse(&kind_text) else {
            return Err(CollabError::Database(format!(
                "change '{seq}' has unknown kind '{kind_text}'"
            )));
        };
        Ok(Self {
            seq,
            kind,
            target_id: row.get(2)?,
            entity_id: row.get(3)?,
            created_at_ms: row.get(4)?,
        })
    }
}

impl CollabCore {
    /// Return one authorization-filtered bootstrap projection and the global
    /// durable change cursor observed in the same connection critical section.
    pub async fn snapshot(&self, actor_id: &str) -> Result<CollabSnapshot> {
        self.read(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let cursor = ChangeStore::new(connection).latest_change_seq().await?;
            let targets = TargetStore::new(connection).for_actor(actor_id).await?;
            let followed_thread_ids = ThreadStore::new(connection)
                .followed_thread_ids(&ActorId::parse(actor_id)?)
                .await?;
            let tasks = TaskStore::new(connection)
                .tasks_for_actor(actor_id, None)
                .await?;
            Ok(CollabSnapshot {
                actor,
                cursor,
                targets,
                followed_thread_ids,
                tasks,
            })
        })
        .await
    }

    /// Return durable changes addressed to one actor after a global cursor.
    pub async fn list_changes(
        &self,
        actor_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ChangeEvent>> {
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 500 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 500".into(),
            ));
        }
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let minimum_cursor = ChangeStore::new(connection)
                .change_retention_floor()
                .await?;
            let maximum_cursor = ChangeStore::new(connection).latest_change_seq().await?;
            if after_seq < minimum_cursor || after_seq > maximum_cursor {
                return Err(CollabError::ChangeCursorOutOfRange {
                    after_seq,
                    minimum_cursor,
                    maximum_cursor,
                });
            }
            ChangeStore::new(connection)
                .changes_after(actor_id, after_seq, limit)
                .await
        })
        .await
    }

    /// Delete the oldest contiguous prefix of change events older than one
    /// wall-clock cutoff and persist the newest cursor that remains safe for
    /// incremental replay. A non-monotonic clock may retain extra old rows,
    /// but can never create a replay hole.
    pub async fn prune_changes_before(&self, before_ms: i64) -> Result<i64> {
        if before_ms < 0 {
            return Err(CollabError::InvalidArgument(
                "before_ms must not be negative".into(),
            ));
        }
        self.write(async |connection| {
            let current_floor = ChangeStore::new(connection)
                .change_retention_floor()
                .await?;
            let first_retained_seq = require_scalar_row(
                connection
                    .query_row::<Option<i64>>(
                        "SELECT MIN(seq) FROM change_events WHERE created_at_ms >= ?1",
                        [before_ms],
                    )
                    .await?,
                "change retention query",
            )?;
            let prune_through = match first_retained_seq {
                Some(first_retained_seq) => first_retained_seq - 1,
                None => ChangeStore::new(connection).latest_change_seq().await?,
            };
            if prune_through <= current_floor {
                return Ok(current_floor);
            }
            let store = ChangeStore::new(connection);
            store.delete_changes_through(prune_through).await?;
            store.set_retention_floor(prune_through).await?;
            Ok(prune_through)
        })
        .await
    }
}

/// The collab_meta key holding the durable change replay floor.
const CHANGE_RETENTION_FLOOR_KEY: &str = "change_retention_floor";

/// Persistence for the change ledger; the only owner of its SQL.
pub(crate) struct ChangeStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ChangeStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// The newest durable change cursor, never below the retention floor.
    pub(crate) async fn latest_change_seq(&self) -> Result<i64> {
        let latest = require_scalar_row(
            self.connection
                .query_row::<i64>("SELECT COALESCE(MAX(seq), 0) FROM change_events", ())
                .await?,
            "change sequence query",
        )?;
        Ok(latest.max(self.change_retention_floor().await?))
    }

    /// The oldest cursor still safe for incremental replay.
    pub(crate) async fn change_retention_floor(&self) -> Result<i64> {
        let value = match self
            .connection
            .query_row::<String>(
                "SELECT value FROM collab_meta WHERE key = ?1",
                [CHANGE_RETENTION_FLOOR_KEY],
            )
            .await?
        {
            Some(value) => value,
            None => return Ok(0),
        };
        let floor = value.parse::<i64>().map_err(|_| {
            CollabError::Database(format!(
                "change retention floor '{value}' is not a signed 64-bit integer"
            ))
        })?;
        if floor < 0 {
            return Err(CollabError::Database(format!(
                "change retention floor '{floor}' is negative"
            )));
        }
        Ok(floor)
    }

    /// Delete the recipients and events of every change through one cursor.
    /// The two deletes stay together: recipients first so no orphaned join
    /// row survives a partial prune.
    pub(crate) async fn delete_changes_through(&self, through_seq: i64) -> Result<()> {
        self.connection
            .execute(
                "DELETE FROM change_recipients WHERE change_seq <= ?1",
                [through_seq],
            )
            .await?;
        self.connection
            .execute("DELETE FROM change_events WHERE seq <= ?1", [through_seq])
            .await?;
        Ok(())
    }

    /// Persist the newest cursor that remains safe for incremental replay.
    pub(crate) async fn set_retention_floor(&self, floor: i64) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (CHANGE_RETENTION_FLOOR_KEY, floor.to_string()),
            )
            .await?;
        Ok(())
    }

    /// Changes addressed to one actor after a global cursor, in sequence order.
    pub(crate) async fn changes_after(
        &self,
        actor_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ChangeEvent>> {
        self.connection
            .query_rows::<ChangeEvent>(
                "SELECT change.seq, change.kind, change.target_id,
                        change.entity_id, change.created_at_ms
                 FROM change_recipients recipient
                 JOIN change_events change ON change.seq = recipient.change_seq
                 WHERE recipient.actor_id = ?1 AND change.seq > ?2
                 ORDER BY change.seq
                 LIMIT ?3",
                (actor_id, after_seq, i64::from(limit)),
            )
            .await
    }

    /// Every durable actor id, in id order, for broadcast changes.
    pub(crate) async fn all_actor_ids(&self) -> Result<Vec<String>> {
        self.connection
            .query_rows::<String>("SELECT id FROM actors ORDER BY id", ())
            .await
    }

    /// Append one change event addressed to an explicit recipient snapshot and
    /// return its global sequence.
    pub(crate) async fn insert_change(
        &self,
        kind: ChangeKind,
        target_id: Option<&str>,
        entity_id: &str,
        recipient_ids: &[String],
        now: i64,
    ) -> Result<i64> {
        let seq = require_scalar_row(
            self.connection
                .query_row::<i64>(
                    "INSERT INTO change_events
                     (kind, target_id, entity_id, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4)
                     RETURNING seq",
                    (kind.as_str(), target_id, entity_id, now),
                )
                .await?,
            "change insert",
        )?;
        for actor_id in recipient_ids {
            self.connection
                .execute(
                    "INSERT INTO change_recipients (change_seq, actor_id)
                     VALUES (?1, ?2)",
                    (seq, actor_id.as_str()),
                )
                .await?;
        }
        Ok(seq)
    }

    /// Append one change event addressed to a target's current membership plus
    /// explicit extra actors.
    pub(crate) async fn insert_target_change(
        &self,
        kind: ChangeKind,
        target_id: &str,
        entity_id: &str,
        extra_actor_ids: &[&str],
        now: i64,
    ) -> Result<i64> {
        let recipients = MembershipStore::new(self.connection)
            .target_change_recipients(target_id, extra_actor_ids)
            .await?;
        self.insert_change(kind, Some(target_id), entity_id, &recipients, now)
            .await
    }
}
