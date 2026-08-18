//! Transactional local collaboration core for the DSH collab plugin.

mod error;
mod model;
#[cfg(feature = "napi")]
mod napi_bridge;
mod schema;

#[cfg(feature = "napi")]
pub use napi_bridge::*;

pub use error::{CollabError, Result};
pub use model::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind,
    Actor, ActorKind, ChangeEvent, ChangeKind, CollabSnapshot, InboxBatch, InboxMessage, Message,
    MessageTail, PendingWake, RuntimeBinding, SendMessageRequest, SendMessageResult, Target,
    TargetKind, Task, TaskStatus,
};

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use turso::transaction::TransactionBehavior;
use turso::{Connection, Row};
use uuid::Uuid;

use crate::schema::{
    META_SCHEMA, SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_VERSION,
};

const CHANGE_RETENTION_FLOOR_KEY: &str = "change_retention_floor";

const ACTIVITY_INBOX_CANDIDATES_CTE: &str = r#"
WITH eligible AS (
  SELECT target.id,
         target.kind,
         target.name,
         target.parent_target_id,
         target.root_message_id,
         COALESCE(done.done_through_seq, 0) AS done_through_seq
  FROM targets AS target
  JOIN memberships AS membership
    ON membership.target_id = CASE
      WHEN target.kind = 'thread' THEN target.parent_target_id
      ELSE target.id
    END
   AND membership.actor_id = ?1
   AND membership.left_at_ms IS NULL
  LEFT JOIN targets AS parent ON parent.id = target.parent_target_id
  LEFT JOIN activity_inbox_done AS done
    ON done.actor_id = ?1 AND done.target_id = target.id
  WHERE target.archived_at_ms IS NULL
    AND target.kind IN ('channel', 'direct', 'thread')
    AND (
      target.kind <> 'thread'
      OR (
        parent.kind IN ('channel', 'direct')
        AND parent.archived_at_ms IS NULL
        AND EXISTS (
          SELECT 1
          FROM thread_follows AS follow
          WHERE follow.thread_target_id = target.id
            AND follow.actor_id = ?1
            AND follow.unfollowed_at_ms IS NULL
        )
      )
    )
), latest_activity AS (
  SELECT target_id, MAX(seq) AS last_activity_seq
  FROM messages
  GROUP BY target_id
), active AS (
  SELECT eligible.id,
         eligible.kind,
         eligible.name,
         eligible.parent_target_id,
         eligible.root_message_id,
         latest_activity.last_activity_seq
  FROM eligible
  JOIN latest_activity ON latest_activity.target_id = eligible.id
  WHERE latest_activity.last_activity_seq > eligible.done_through_seq
)
"#;

const ACTIVITY_INBOX_ITEMS_SELECT: &str = r#"
SELECT active.id,
       active.kind,
       active.parent_target_id,
       active.root_message_id,
       active.last_activity_seq,
       CASE active.kind
         WHEN 'channel' THEN active.name
         WHEN 'direct' THEN COALESCE(direct_peer.display_name, direct_peer.handle, active.name)
         ELSE CASE parent.kind
           WHEN 'channel' THEN parent.name
           ELSE COALESCE(parent_direct_peer.display_name, parent_direct_peer.handle, parent.name)
         END
       END AS target_name,
       CASE active.kind
         WHEN 'thread' THEN root.body_json
         ELSE latest.body_json
       END AS title_body_json,
       latest_author.kind AS latest_author_kind,
       latest_author.display_name AS latest_author_name,
       latest.body_json AS latest_body_json,
       latest.created_at_ms AS last_activity_at_ms,
       CASE active.kind
         WHEN 'thread' THEN (
           SELECT COUNT(*) FROM messages AS reply WHERE reply.target_id = active.id
         )
         ELSE NULL
       END AS reply_count,
       task.number AS task_number,
       task.status AS task_status,
       assignee.display_name AS task_assignee_name
FROM active
JOIN messages AS latest
  ON latest.target_id = active.id AND latest.seq = active.last_activity_seq
JOIN actors AS latest_author ON latest_author.id = latest.author_id
LEFT JOIN messages AS root ON root.id = active.root_message_id
LEFT JOIN targets AS parent ON parent.id = active.parent_target_id
LEFT JOIN direct_pairs AS direct_pair ON direct_pair.target_id = active.id
LEFT JOIN actors AS direct_peer ON direct_peer.id = CASE
  WHEN direct_pair.actor_low_id = ?1 THEN direct_pair.actor_high_id
  WHEN direct_pair.actor_high_id = ?1 THEN direct_pair.actor_low_id
  ELSE NULL
END
LEFT JOIN direct_pairs AS parent_direct_pair ON parent_direct_pair.target_id = parent.id
LEFT JOIN actors AS parent_direct_peer ON parent_direct_peer.id = CASE
  WHEN parent_direct_pair.actor_low_id = ?1 THEN parent_direct_pair.actor_high_id
  WHEN parent_direct_pair.actor_high_id = ?1 THEN parent_direct_pair.actor_low_id
  ELSE NULL
END
LEFT JOIN tasks AS task ON task.message_id = CASE
  WHEN active.kind = 'thread' THEN active.root_message_id
  ELSE latest.id
END
LEFT JOIN actors AS assignee ON assignee.id = task.assignee_id
"#;

/// One process-local handle over the authoritative local Turso database.
pub struct CollabCore {
    connection: Mutex<Connection>,
    closed: AtomicBool,
}

impl CollabCore {
    /// Open a local Turso file and apply the current schema.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        prepare_database_parent(path)?;
        let database_path = path
            .to_str()
            .ok_or_else(|| CollabError::InvalidArgument("database path must be UTF-8".into()))?;
        let database = turso::Builder::new_local(database_path).build().await?;
        let mut connection = database.connect()?;
        migrate(&mut connection).await?;
        protect_database_file(path)?;
        Ok(Self {
            connection: Mutex::new(connection),
            closed: AtomicBool::new(false),
        })
    }

    /// Open an in-memory Turso database for tests and ephemeral hosts.
    pub async fn open_memory() -> Result<Self> {
        Self::open(Path::new(":memory:")).await
    }

    /// Stop admitting new operations. In-flight operations hold the connection
    /// mutex and finish before this future returns.
    pub async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        let _connection = self.connection.lock().await;
        Ok(())
    }

    /// Create the one local human/user actor.
    pub async fn create_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        self.create_actor(ActorKind::User, handle, display_name, None)
            .await
    }

    /// Create a stable Agent actor and record its durable workspace path.
    pub async fn create_agent(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
    ) -> Result<Actor> {
        require_non_empty("workspace_path", workspace_path)?;
        self.create_actor(ActorKind::Agent, handle, display_name, Some(workspace_path))
            .await
    }

    /// Delete one Agent's operational state. The actor row and its messages
    /// stay so history never points at a missing author.
    pub async fn delete_agent(&self, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let actor = find_actor(&transaction, actor_id).await?;
        if actor.kind != ActorKind::Agent {
            return Err(not_found("agent", actor_id));
        }
        transaction
            .execute("DELETE FROM memberships WHERE actor_id = ?1", [actor_id])
            .await?;
        transaction
            .execute(
                "DELETE FROM runtime_bindings WHERE agent_id = ?1",
                [actor_id],
            )
            .await?;
        transaction
            .execute("DELETE FROM agents WHERE actor_id = ?1", [actor_id])
            .await?;
        transaction
            .execute(
                "DELETE FROM agent_wake_state WHERE agent_id = ?1",
                [actor_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM inbox_batch_items
                 WHERE batch_id IN (SELECT id FROM inbox_batches WHERE agent_id = ?1)",
                [actor_id],
            )
            .await?;
        transaction
            .execute("DELETE FROM inbox_batches WHERE agent_id = ?1", [actor_id])
            .await?;
        // The actor set changed; reuse actor_created so clients re-pull actors.
        let actor_ids = all_actor_ids(&transaction).await?;
        insert_change(
            &transaction,
            ChangeKind::ActorCreated,
            None,
            &actor.id,
            &actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Return the stable User for one handle, creating it when absent.
    pub async fn ensure_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        self.assert_open()?;
        require_non_empty("handle", handle)?;
        require_non_empty("display_name", display_name)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        if let Some(actor) = find_actor_by_handle(&transaction, handle).await? {
            if actor.kind != ActorKind::User {
                return Err(CollabError::InvalidArgument(format!(
                    "actor handle '{handle}' belongs to an Agent"
                )));
            }
            if actor.display_name != display_name {
                // Only leftover default names migrate. A custom display name
                // is user-owned and must survive later OS-username ensure.
                if actor.display_name != "Local User" {
                    transaction.commit().await?;
                    return Ok(actor);
                }
                // Explicit display-name migration on the stable handle: the
                // actor id, Memberships and Tasks are untouched.
                transaction
                    .execute(
                        "UPDATE actors SET display_name = ?2 WHERE id = ?1",
                        (actor.id.as_str(), display_name),
                    )
                    .await?;
                let actor_ids = all_actor_ids(&transaction).await?;
                insert_change(
                    &transaction,
                    ChangeKind::ActorCreated,
                    None,
                    &actor.id,
                    &actor_ids,
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Ok(Actor {
                    display_name: display_name.to_owned(),
                    ..actor
                });
            }
            transaction.commit().await?;
            return Ok(actor);
        }

        let actor = Actor {
            id: new_id(),
            kind: ActorKind::User,
            handle: handle.to_owned(),
            display_name: display_name.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, 'user', ?2, ?3, ?4)",
                (
                    actor.id.as_str(),
                    actor.handle.as_str(),
                    actor.display_name.as_str(),
                    now,
                ),
            )
            .await?;
        let actor_ids = all_actor_ids(&transaction).await?;
        insert_change(
            &transaction,
            ChangeKind::ActorCreated,
            None,
            &actor.id,
            &actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(actor)
    }

    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(&self, name: &str, creator_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("channel name", name)?;
        require_non_empty("creator_id", creator_id)?;

        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.to_owned(),
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.to_owned(),
            created_at_ms: now,
        };

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, creator_id).await?;
        transaction
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
        transaction
            .execute(
                "INSERT INTO memberships
                 (target_id, actor_id, role, joined_at_ms, left_at_ms)
                 VALUES (?1, ?2, 'owner', ?3, NULL)",
                (target.id.as_str(), target.created_by.as_str(), now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Return the one stable Direct target for an unordered pair of actors,
    /// creating it and its two memberships when absent.
    pub async fn create_direct(&self, actor_id: &str, peer_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        require_non_empty("peer_id", peer_id)?;
        if actor_id == peer_id {
            return Err(CollabError::InvalidArgument(
                "a Direct target requires two distinct actors".into(),
            ));
        }
        let (actor_low_id, actor_high_id) = if actor_id < peer_id {
            (actor_id, peer_id)
        } else {
            (peer_id, actor_id)
        };
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        require_actor(&transaction, peer_id).await?;

        let mut rows = transaction
            .query(
                "SELECT target_id FROM direct_pairs
                 WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                (actor_low_id, actor_high_id),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let target_id = row.get::<String>(0)?;
            drop(rows);
            let target = find_target(&transaction, &target_id).await?;
            transaction.commit().await?;
            return Ok(target);
        }
        drop(rows);

        let low_handle = actor_handle(&transaction, actor_low_id).await?;
        let high_handle = actor_handle(&transaction, actor_high_id).await?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Direct,
            name: format!("@{low_handle} ↔ @{high_handle}"),
            parent_target_id: None,
            root_message_id: None,
            created_by: actor_id.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, 'direct', ?2, NULL, NULL, ?3, ?4, NULL)",
                (
                    target.id.as_str(),
                    target.name.as_str(),
                    target.created_by.as_str(),
                    now,
                ),
            )
            .await?;
        for (member_id, role) in [(actor_id, "owner"), (peer_id, "member")] {
            transaction
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, ?3, ?4, NULL)",
                    (target.id.as_str(), member_id, role, now),
                )
                .await?;
        }
        transaction
            .execute(
                "INSERT INTO direct_pairs
                 (target_id, actor_low_id, actor_high_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                (target.id.as_str(), actor_low_id, actor_high_id, now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Return the one Thread target rooted at a top-level Message. The creator
    /// and root author follow it immediately when they retain parent access.
    pub async fn create_thread(&self, root_message_id: &str, actor_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("root_message_id", root_message_id)?;
        require_non_empty("actor_id", actor_id)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let (parent_target_id, root_author_id) =
            message_target_author(&transaction, root_message_id).await?;
        if require_target(&transaction, &parent_target_id).await? == TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "Threads cannot be nested under Thread messages".into(),
            ));
        }
        require_active_member(
            &transaction,
            &parent_target_id,
            actor_id,
            "create Thread in",
        )
        .await?;

        let mut rows = transaction
            .query(
                "SELECT id FROM targets WHERE root_message_id = ?1",
                [root_message_id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let target_id = row.get::<String>(0)?;
            drop(rows);
            let target = find_target(&transaction, &target_id).await?;
            transaction.commit().await?;
            return Ok(target);
        }
        drop(rows);

        let target = Target {
            id: new_id(),
            kind: TargetKind::Thread,
            name: format!("thread:{root_message_id}"),
            parent_target_id: Some(parent_target_id.clone()),
            root_message_id: Some(root_message_id.to_owned()),
            created_by: actor_id.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, 'thread', ?2, ?3, ?4, ?5, ?6, NULL)",
                (
                    target.id.as_str(),
                    target.name.as_str(),
                    parent_target_id.as_str(),
                    root_message_id,
                    actor_id,
                    now,
                ),
            )
            .await?;
        follow_thread_in_transaction(&transaction, &target.id, actor_id, now).await?;
        if root_author_id != actor_id
            && is_active_member(&transaction, &parent_target_id, &root_author_id).await?
        {
            follow_thread_in_transaction(&transaction, &target.id, &root_author_id, now).await?;
        }
        let parent_actor_ids = active_member_ids(&transaction, &parent_target_id).await?;
        insert_change(
            &transaction,
            ChangeKind::TargetCreated,
            Some(&target.id),
            &target.id,
            &parent_actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Follow one Thread after rechecking access to its parent target.
    pub async fn follow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route = require_target_route(&transaction, thread_target_id).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "follow_thread requires a Thread target".into(),
            ));
        }
        require_active_member(
            &transaction,
            route.permission_target_id(thread_target_id),
            actor_id,
            "follow",
        )
        .await?;
        if follow_thread_in_transaction(&transaction, thread_target_id, actor_id, now).await? {
            insert_target_change(
                &transaction,
                ChangeKind::ThreadFollowChanged,
                thread_target_id,
                actor_id,
                &[actor_id],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Stop future ordinary Thread delivery for one current parent member.
    pub async fn unfollow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route = require_target_route(&transaction, thread_target_id).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "unfollow_thread requires a Thread target".into(),
            ));
        }
        require_active_member(
            &transaction,
            route.permission_target_id(thread_target_id),
            actor_id,
            "unfollow",
        )
        .await?;
        let changed = transaction
            .execute(
                "UPDATE thread_follows
                 SET unfollowed_at_ms = ?3
                 WHERE thread_target_id = ?1 AND actor_id = ?2
                   AND unfollowed_at_ms IS NULL",
                (thread_target_id, actor_id, now),
            )
            .await?;
        if changed == 1 {
            insert_target_change(
                &transaction,
                ChangeKind::ThreadFollowChanged,
                thread_target_id,
                actor_id,
                &[actor_id],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Add or reactivate one Channel member.
    pub async fn add_member(&self, target_id: &str, actor_id: &str, added_by: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let actor_kind = require_actor(&transaction, actor_id).await?;
        if require_target(&transaction, target_id).await? != TargetKind::Channel {
            return Err(CollabError::InvalidArgument(
                "add_member only supports Channel targets".into(),
            ));
        }
        require_owner(&transaction, target_id, added_by).await?;
        transaction
            .execute(
                "INSERT INTO memberships
                 (target_id, actor_id, role, joined_at_ms, left_at_ms)
                 VALUES (?1, ?2, 'member', ?3, NULL)
                 ON CONFLICT(target_id, actor_id) DO UPDATE SET
                   role = CASE
                     WHEN memberships.role = 'owner' THEN 'owner'
                     ELSE 'member'
                   END,
                   joined_at_ms = excluded.joined_at_ms,
                   left_at_ms = NULL",
                (target_id, actor_id, now),
            )
            .await?;
        if actor_kind == ActorKind::Agent {
            transaction
                .execute(
                    "UPDATE agent_wake_state
                     SET notified_generation = 0
                     WHERE agent_id = ?1",
                    [actor_id],
                )
                .await?;
        }
        insert_target_change(
            &transaction,
            ChangeKind::MembershipChanged,
            target_id,
            actor_id,
            &[actor_id],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// List the local actor directory after authenticating the caller. A
    /// deleted Agent keeps its actors row for message authorship but loses its
    /// agents row, so the directory lists users and live Agents only.
    pub async fn list_actors(&self, actor_id: &str) -> Result<Vec<Actor>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        let mut rows = connection
            .query(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors
                 WHERE kind = 'user' OR id IN (SELECT actor_id FROM agents)
                 ORDER BY handle, id",
                (),
            )
            .await?;
        let mut actors = Vec::new();
        while let Some(row) = rows.next().await? {
            actors.push(actor_from_row(&row)?);
        }
        Ok(actors)
    }

    /// List the active members of one Channel after authorizing the caller's
    /// own access to that Channel. This is the membership projection the web
    /// members pane must use; the actor directory is not a member list.
    pub async fn list_target_members(&self, actor_id: &str, target_id: &str) -> Result<Vec<Actor>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id, "list members of").await?;
        if require_target(&connection, target_id).await? != TargetKind::Channel {
            return Err(CollabError::InvalidArgument(
                "list_target_members only supports Channel targets".into(),
            ));
        }
        let mut rows = connection
            .query(
                "SELECT actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms
                 FROM memberships membership
                 JOIN actors actor ON actor.id = membership.actor_id
                 WHERE membership.target_id = ?1
                   AND membership.left_at_ms IS NULL
                 ORDER BY actor.handle, actor.id",
                (target_id,),
            )
            .await?;
        let mut members = Vec::new();
        while let Some(row) = rows.next().await? {
            members.push(actor_from_row(&row)?);
        }
        Ok(members)
    }

    /// Return one authorization-filtered bootstrap projection and the global
    /// durable change cursor observed in the same connection critical section.
    pub async fn snapshot(&self, actor_id: &str) -> Result<CollabSnapshot> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        let actor = find_actor(&connection, actor_id).await?;
        let cursor = latest_change_seq(&connection).await?;
        let targets = targets_for_actor(&connection, actor_id).await?;
        let followed_thread_ids = followed_thread_ids_for_actor(&connection, actor_id).await?;
        let tasks = tasks_for_actor(&connection, actor_id, None).await?;
        Ok(CollabSnapshot {
            actor,
            cursor,
            targets,
            followed_thread_ids,
            tasks,
        })
    }

    /// Return durable changes addressed to one actor after a global cursor.
    pub async fn list_changes(
        &self,
        actor_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ChangeEvent>> {
        self.assert_open()?;
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
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        let minimum_cursor = change_retention_floor(&connection).await?;
        let maximum_cursor = latest_change_seq(&connection).await?;
        if after_seq < minimum_cursor || after_seq > maximum_cursor {
            return Err(CollabError::ChangeCursorOutOfRange {
                after_seq,
                minimum_cursor,
                maximum_cursor,
            });
        }
        let mut rows = connection
            .query(
                "SELECT change.seq, change.kind, change.target_id,
                        change.entity_id, change.created_at_ms
                 FROM change_recipients recipient
                 JOIN change_events change ON change.seq = recipient.change_seq
                 WHERE recipient.actor_id = ?1 AND change.seq > ?2
                 ORDER BY change.seq
                 LIMIT ?3",
                (actor_id, after_seq, i64::from(limit)),
            )
            .await?;
        let mut changes = Vec::new();
        while let Some(row) = rows.next().await? {
            changes.push(change_from_row(&row)?);
        }
        Ok(changes)
    }

    /// Delete the oldest contiguous prefix of change events older than one
    /// wall-clock cutoff and persist the newest cursor that remains safe for
    /// incremental replay. A non-monotonic clock may retain extra old rows,
    /// but can never create a replay hole.
    pub async fn prune_changes_before(&self, before_ms: i64) -> Result<i64> {
        self.assert_open()?;
        if before_ms < 0 {
            return Err(CollabError::InvalidArgument(
                "before_ms must not be negative".into(),
            ));
        }
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let current_floor = change_retention_floor(&transaction).await?;
        let mut rows = transaction
            .query(
                "SELECT MIN(seq) FROM change_events WHERE created_at_ms >= ?1",
                [before_ms],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::Database(
                "change retention query returned no row".into(),
            ));
        };
        let first_retained_seq = row.get::<Option<i64>>(0)?;
        drop(rows);
        let prune_through = match first_retained_seq {
            Some(first_retained_seq) => first_retained_seq - 1,
            None => latest_change_seq(&transaction).await?,
        };
        if prune_through <= current_floor {
            transaction.commit().await?;
            return Ok(current_floor);
        }

        transaction
            .execute(
                "DELETE FROM change_recipients WHERE change_seq <= ?1",
                [prune_through],
            )
            .await?;
        transaction
            .execute("DELETE FROM change_events WHERE seq <= ?1", [prune_through])
            .await?;
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (CHANGE_RETENTION_FLOOR_KEY, prune_through.to_string()),
            )
            .await?;
        transaction.commit().await?;
        Ok(prune_through)
    }

    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        if let Some(target_id) = target_id {
            require_target_access(&connection, target_id, actor_id, "list tasks in").await?;
        }
        tasks_for_actor(&connection, actor_id, target_id).await
    }

    /// Atomically commit one immutable Message, its recipient snapshot, and
    /// every recipient Agent's level-triggered wake watermark.
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<SendMessageResult> {
        self.send_message_inner(request, SendFailpoint::None).await
    }

    /// Bind a new DSH Session generation to a stable Agent.
    pub async fn bind_runtime(
        &self,
        agent_id: &str,
        session_id: &str,
        provider: &str,
        model: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        self.assert_open()?;
        for (name, value) in [
            ("agent_id", agent_id),
            ("session_id", session_id),
            ("provider", provider),
            ("model", model),
            ("preset", preset),
        ] {
            require_non_empty(name, value)?;
        }

        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_agent(&transaction, agent_id).await?;
        let generation = current_generation(&transaction, agent_id).await? + 1;
        transaction
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
                    agent_id, session_id, generation, provider, model, preset, now,
                ),
            )
            .await?;
        transaction.commit().await?;

        Ok(RuntimeBinding {
            agent_id: agent_id.to_owned(),
            session_id: session_id.to_owned(),
            generation,
            provider: provider.to_owned(),
            model: model.to_owned(),
            preset: preset.to_owned(),
            bound_at_ms: now,
        })
    }

    /// Return the current runtime binding for one stable Agent.
    pub async fn runtime_binding(&self, agent_id: &str) -> Result<Option<RuntimeBinding>> {
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        let connection = self.connection.lock().await;
        find_runtime_binding(&connection, "agent_id", agent_id).await
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
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        require_non_empty("session_id", session_id)?;
        require_non_empty("preset", preset)?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let mut rows = transaction
            .query(
                "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
                 FROM runtime_bindings WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "runtime binding",
                id: agent_id.to_owned(),
            });
        };
        let current = RuntimeBinding {
            agent_id: row.get(0)?,
            session_id: row.get(1)?,
            generation: row.get(2)?,
            provider: row.get(3)?,
            model: row.get(4)?,
            preset: row.get(5)?,
            bound_at_ms: row.get(6)?,
        };
        drop(rows);
        if current.generation != generation || current.session_id != session_id {
            return Err(CollabError::RuntimeGenerationMismatch {
                agent_id: agent_id.to_owned(),
            });
        }
        transaction
            .execute(
                "UPDATE runtime_bindings SET preset = ?1
                 WHERE agent_id = ?2 AND generation = ?3 AND session_id = ?4",
                (preset, agent_id, generation, session_id),
            )
            .await?;
        transaction.commit().await?;
        Ok(RuntimeBinding {
            preset: preset.to_owned(),
            ..current
        })
    }

    /// Resolve the stable Agent identity that owns one live DSH Session id.
    pub async fn runtime_binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RuntimeBinding>> {
        self.assert_open()?;
        require_non_empty("session_id", session_id)?;
        let connection = self.connection.lock().await;
        find_runtime_binding(&connection, "session_id", session_id).await
    }

    /// List every durable current runtime binding for process recovery.
    pub async fn list_runtime_bindings(&self) -> Result<Vec<RuntimeBinding>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
                 FROM runtime_bindings ORDER BY agent_id",
                (),
            )
            .await?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().await? {
            bindings.push(RuntimeBinding {
                agent_id: row.get(0)?,
                session_id: row.get(1)?,
                generation: row.get(2)?,
                provider: row.get(3)?,
                model: row.get(4)?,
                preset: row.get(5)?,
                bound_at_ms: row.get(6)?,
            });
        }
        Ok(bindings)
    }

    /// Scan the level-triggered wake ledger. Only authorized deliveries that
    /// have not reached a model request contribute to the returned watermark.
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<PendingWake>> {
        self.assert_open()?;
        if limit == 0 || limit > 1000 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 1000".into(),
            ));
        }
        let connection = self.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT rb.agent_id, rb.session_id, rb.generation,
                        rb.provider, rb.model, rb.preset, rb.bound_at_ms,
                        pending.pending_seq
                 FROM runtime_bindings rb
                 JOIN agent_wake_state wake ON wake.agent_id = rb.agent_id
                 JOIN (
                   SELECT d.recipient_id AS agent_id, MAX(d.message_seq) AS pending_seq
                   FROM deliveries d
                   JOIN targets target ON target.id = d.target_id
                   JOIN memberships mem
                     ON mem.target_id = CASE
                       WHEN target.kind = 'thread' THEN target.parent_target_id
                       ELSE target.id
                     END
                    AND mem.actor_id = d.recipient_id
                    AND mem.left_at_ms IS NULL
                   WHERE d.model_seen_at_ms IS NULL
                   GROUP BY d.recipient_id
                 ) pending ON pending.agent_id = rb.agent_id
                 WHERE pending.pending_seq > wake.notified_seq
                    OR wake.notified_generation <> rb.generation
                    OR EXISTS (
                      SELECT 1 FROM deliveries d
                      JOIN targets target ON target.id = d.target_id
                      JOIN memberships mem
                        ON mem.target_id = CASE
                          WHEN target.kind = 'thread' THEN target.parent_target_id
                          ELSE target.id
                        END
                       AND mem.actor_id = d.recipient_id
                       AND mem.left_at_ms IS NULL
                      WHERE d.recipient_id = rb.agent_id
                        AND d.model_seen_at_ms IS NULL
                        AND (d.notified_at_ms IS NULL
                          OR d.notified_generation <> rb.generation)
                    )
                 ORDER BY pending.pending_seq, rb.agent_id
                 LIMIT ?1",
                [i64::from(limit)],
            )
            .await?;
        let mut wakes = Vec::new();
        while let Some(row) = rows.next().await? {
            wakes.push(PendingWake {
                binding: RuntimeBinding {
                    agent_id: row.get(0)?,
                    session_id: row.get(1)?,
                    generation: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    preset: row.get(5)?,
                    bound_at_ms: row.get(6)?,
                },
                pending_seq: row.get(7)?,
            });
        }
        Ok(wakes)
    }

    /// Record that the exact current Session generation accepted a content-free
    /// wake through `pending_seq`. A concurrently committed newer Message stays
    /// above this watermark and will be returned by the next scan.
    pub async fn mark_notified(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        pending_seq: i64,
    ) -> Result<()> {
        self.assert_open()?;
        if pending_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "pending_seq must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
            .query(
                "SELECT pending_seq FROM agent_wake_state WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(not_found("wake state", agent_id));
        };
        let durable_pending = row.get::<i64>(0)?;
        drop(rows);
        if pending_seq > durable_pending {
            return Err(CollabError::InvalidArgument(format!(
                "pending_seq {pending_seq} exceeds durable watermark {durable_pending}"
            )));
        }

        transaction
            .execute(
                "UPDATE agent_wake_state
                 SET notified_seq = MAX(notified_seq, ?2),
                     notified_generation = ?3,
                     attempt_count = 0,
                     next_retry_at_ms = NULL,
                     last_error = NULL
                 WHERE agent_id = ?1",
                (agent_id, pending_seq, generation),
            )
            .await?;
        transaction
            .execute(
                "UPDATE deliveries
                 SET notified_at_ms = ?3, notified_generation = ?4
                 WHERE recipient_id = ?1
                   AND message_seq <= ?2
                   AND model_seen_at_ms IS NULL
                   AND EXISTS (
                     SELECT 1 FROM targets target
                     JOIN memberships mem
                       ON mem.target_id = CASE
                         WHEN target.kind = 'thread' THEN target.parent_target_id
                         ELSE target.id
                       END
                     WHERE target.id = deliveries.target_id
                       AND mem.actor_id = deliveries.recipient_id
                       AND mem.left_at_ms IS NULL
                   )",
                (agent_id, pending_seq, now, generation),
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Re-arm model-unseen work when the same persisted Session is resumed into
    /// a fresh process-local Agent inbox. This does not create a new Session
    /// generation; the current binding still fences the operation.
    pub async fn rearm_runtime_wake(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        self.assert_open()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;
        transaction
            .execute(
                "UPDATE agent_wake_state
                 SET notified_generation = 0
                 WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Return the authorized, not-yet-model-seen deliveries for the current
    /// runtime generation and persist the exact returned batch.
    pub async fn check_inbox(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        limit: u32,
    ) -> Result<InboxBatch> {
        self.assert_open()?;
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
            .query(
                "SELECT d.id, m.seq, m.id, m.target_id, m.author_id,
                        m.client_request_id, m.body_json, m.created_at_ms
                 FROM deliveries d
                 JOIN messages m ON m.id = d.message_id
                 JOIN targets target ON target.id = m.target_id
                 JOIN memberships mem
                   ON mem.target_id = CASE
                     WHEN target.kind = 'thread' THEN target.parent_target_id
                     ELSE target.id
                   END
                  AND mem.actor_id = ?1
                 WHERE d.recipient_id = ?1
                   AND d.model_seen_at_ms IS NULL
                   AND mem.left_at_ms IS NULL
                 ORDER BY m.seq
                 LIMIT ?2",
                (agent_id, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            let delivery_id = row.get::<String>(0)?;
            let body_json = row.get::<String>(6)?;
            let body: StoredTextBody = serde_json::from_str(&body_json).map_err(|error| {
                CollabError::Database(format!("message body is malformed: {error}"))
            })?;
            messages.push(InboxMessage {
                delivery_id,
                message: Message {
                    seq: row.get(1)?,
                    id: row.get(2)?,
                    target_id: row.get(3)?,
                    author_id: row.get(4)?,
                    client_request_id: row.get(5)?,
                    text: body.text,
                    created_at_ms: row.get(7)?,
                },
            });
        }
        drop(rows);

        if messages.is_empty() {
            transaction.commit().await?;
            return Ok(InboxBatch {
                id: None,
                agent_id: agent_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
                messages,
                checked_at_ms: now,
            });
        }

        let batch_id = new_id();
        transaction
            .execute(
                "INSERT INTO inbox_batches
                 (id, agent_id, session_id, generation, checked_at_ms, model_seen_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                (batch_id.as_str(), agent_id, session_id, generation, now),
            )
            .await?;
        for item in &messages {
            transaction
                .execute(
                    "INSERT INTO inbox_batch_items (batch_id, delivery_id) VALUES (?1, ?2)",
                    (batch_id.as_str(), item.delivery_id.as_str()),
                )
                .await?;
            transaction
                .execute(
                    "UPDATE deliveries
                     SET checked_at_ms = COALESCE(checked_at_ms, ?2)
                     WHERE id = ?1",
                    (item.delivery_id.as_str(), now),
                )
                .await?;
        }
        transaction.commit().await?;

        Ok(InboxBatch {
            id: Some(batch_id),
            agent_id: agent_id.to_owned(),
            session_id: session_id.to_owned(),
            generation,
            messages,
            checked_at_ms: now,
        })
    }

    /// Mark one check batch model-seen only if its Session generation is still
    /// the current runtime binding.
    pub async fn mark_model_seen(
        &self,
        batch_id: &str,
        agent_id: &str,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
            .query(
                "SELECT agent_id, session_id, generation
                 FROM inbox_batches WHERE id = ?1",
                [batch_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(not_found("inbox batch", batch_id));
        };
        let batch_agent = row.get::<String>(0)?;
        let batch_session = row.get::<String>(1)?;
        let batch_generation = row.get::<i64>(2)?;
        drop(rows);
        if batch_agent != agent_id || batch_session != session_id || batch_generation != generation
        {
            return Err(CollabError::RuntimeGenerationMismatch {
                agent_id: agent_id.to_owned(),
            });
        }

        transaction
            .execute(
                "UPDATE deliveries
                 SET model_seen_at_ms = COALESCE(model_seen_at_ms, ?2),
                     seen_generation = COALESCE(seen_generation, ?3),
                     seen_session_id = COALESCE(seen_session_id, ?4)
                 WHERE id IN (
                   SELECT delivery_id FROM inbox_batch_items WHERE batch_id = ?1
                 )",
                (batch_id, now, generation, session_id),
            )
            .await?;
        transaction
            .execute(
                "UPDATE inbox_batches
                 SET model_seen_at_ms = COALESCE(model_seen_at_ms, ?2)
                 WHERE id = ?1",
                (batch_id, now),
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Read one exact Message while the actor retains access to the exact
    /// target, inherited from the parent for a Thread.
    pub async fn read_message(
        &self,
        actor_id: &str,
        target_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id, "read").await?;
        let mut rows = connection
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM messages WHERE id = ?1 AND target_id = ?2",
                (message_id, target_id),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(not_found("message in exact target", message_id));
        };
        message_from_row(&row)
    }

    /// Read an ascending page from one exact target after a global sequence.
    pub async fn read_messages(
        &self,
        actor_id: &str,
        target_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        self.assert_open()?;
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id, "read").await?;
        let mut rows = connection
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM messages
                 WHERE target_id = ?1 AND seq > ?2
                 ORDER BY seq
                 LIMIT ?3",
                (target_id, after_seq, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(message_from_row(&row)?);
        }
        Ok(messages)
    }

    /// Read the exact total count and the latest `limit` messages (ascending)
    /// of one exact target under a single locked snapshot. Unlike an
    /// `after_seq = 0` page, `count` is never a lower bound and the returned
    /// messages are always the true tail.
    pub async fn read_messages_tail(
        &self,
        actor_id: &str,
        target_id: &str,
        limit: u32,
    ) -> Result<MessageTail> {
        self.assert_open()?;
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let mut connection = self.connection.lock().await;
        // Explicit read transaction: COUNT and the tail page observe one DB
        // snapshot even when a second connection writes concurrently.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        require_target_access(&transaction, target_id, actor_id, "read").await?;
        let mut count_rows = transaction
            .query(
                "SELECT COUNT(*) FROM messages WHERE target_id = ?1",
                (target_id,),
            )
            .await?;
        let count_row = count_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
        let count: i64 = count_row.get(0)?;
        drop(count_rows);
        let mut rows = transaction
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM (
                     SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                     FROM messages
                     WHERE target_id = ?1
                     ORDER BY seq DESC
                     LIMIT ?2
                 )
                 ORDER BY seq",
                (target_id, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(message_from_row(&row)?);
        }
        drop(rows);
        transaction.commit().await?;
        Ok(MessageTail { count, messages })
    }

    /// List active Activity conversations newest-first. Eligibility and Done
    /// are actor-specific; count and page are read from one database snapshot.
    pub async fn inbox_list(
        &self,
        actor_id: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ActivityInboxPage> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        if limit == 0 || limit > 50 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 50".into(),
            ));
        }
        let cursor = cursor.map(parse_activity_cursor).transpose()?;
        let cursor_seq = cursor.as_ref().map_or(0, |cursor| cursor.last_activity_seq);
        let cursor_target_id = cursor
            .as_ref()
            .map_or("", |cursor| cursor.conversation_id.as_str());

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        require_actor(&transaction, actor_id).await?;

        let count_statement =
            format!("{ACTIVITY_INBOX_CANDIDATES_CTE} SELECT COUNT(*) FROM active");
        let mut count_rows = transaction.query(count_statement, [actor_id]).await?;
        let count_row = count_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("Activity count returned no row".into()))?;
        let active_count = count_row.get::<i64>(0)?;
        drop(count_rows);

        let page_statement = format!(
            "{ACTIVITY_INBOX_CANDIDATES_CTE}{ACTIVITY_INBOX_ITEMS_SELECT}
             WHERE (?2 = 0)
                OR active.last_activity_seq < ?2
                OR (active.last_activity_seq = ?2 AND active.id > ?3)
             ORDER BY active.last_activity_seq DESC, active.id ASC
             LIMIT ?4"
        );
        let mut rows = transaction
            .query(
                page_statement,
                (actor_id, cursor_seq, cursor_target_id, i64::from(limit) + 1),
            )
            .await?;
        let mut items = Vec::with_capacity(limit as usize + 1);
        while let Some(row) = rows.next().await? {
            items.push(activity_inbox_item_from_row(&row)?);
        }
        drop(rows);
        transaction.commit().await?;

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_cursor = has_more
            .then(|| items.last().map(activity_cursor_for_item))
            .flatten();
        Ok(ActivityInboxPage {
            items,
            next_cursor,
            active_count,
        })
    }

    /// Record an actor's Activity disposition fence. The fence only advances;
    /// a newer Message sequence automatically makes the conversation active.
    pub async fn inbox_done(
        &self,
        actor_id: &str,
        target_id: &str,
        through_seq: i64,
    ) -> Result<()> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        require_non_empty("target_id", target_id)?;
        if through_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "through_seq must be a positive integer".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route =
            require_target_access(&transaction, target_id, actor_id, "mark Activity Done in")
                .await?;
        if route.kind == TargetKind::Thread
            && !is_following_thread(&transaction, target_id, actor_id).await?
        {
            return Err(not_found("active Thread follow", target_id));
        }

        let mut latest_rows = transaction
            .query(
                "SELECT MAX(seq) FROM messages WHERE target_id = ?1",
                [target_id],
            )
            .await?;
        let latest_seq = latest_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("latest Activity query returned no row".into()))?
            .get::<Option<i64>>(0)?
            .ok_or_else(|| not_found("target activity", target_id))?;
        drop(latest_rows);
        if through_seq > latest_seq {
            return Err(CollabError::InvalidArgument(format!(
                "through_seq {through_seq} is newer than target activity {latest_seq}"
            )));
        }

        let changed = transaction
            .execute(
                "INSERT INTO activity_inbox_done
                 (actor_id, target_id, done_through_seq, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(actor_id, target_id) DO UPDATE SET
                   done_through_seq = excluded.done_through_seq,
                   updated_at_ms = excluded.updated_at_ms
                 WHERE excluded.done_through_seq > activity_inbox_done.done_through_seq",
                (actor_id, target_id, through_seq, now),
            )
            .await?;
        if changed == 1 {
            insert_change(
                &transaction,
                ChangeKind::ActivityDoneChanged,
                Some(target_id),
                target_id,
                &[actor_id.to_owned()],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Convert a committed top-level Message to a Task with a target-local
    /// monotonic task number.
    pub async fn create_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let target_id = message_target(&transaction, message_id).await?;
        let route =
            require_target_access(&transaction, &target_id, actor_id, "create task").await?;
        if route.kind == TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "Thread replies cannot become Tasks".into(),
            ));
        }

        if let Some(task) = find_task(&transaction, message_id).await? {
            transaction.commit().await?;
            return Ok(task);
        }

        transaction
            .execute(
                "INSERT INTO target_counters (target_id, next_task_number)
                 VALUES (?1, 1)
                 ON CONFLICT(target_id) DO NOTHING",
                [target_id.as_str()],
            )
            .await?;
        let mut rows = transaction
            .query(
                "SELECT next_task_number FROM target_counters WHERE target_id = ?1",
                [target_id.as_str()],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::Database(
                "target task counter disappeared inside transaction".into(),
            ));
        };
        let number = row.get::<i64>(0)?;
        drop(rows);
        transaction
            .execute(
                "UPDATE target_counters SET next_task_number = ?2 WHERE target_id = ?1",
                (target_id.as_str(), number + 1),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO tasks
                 (message_id, target_id, number, status, assignee_id, version, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, 'todo', NULL, 1, ?4, ?4)",
                (message_id, target_id.as_str(), number, now),
            )
            .await?;
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "created",
            None,
            Some(TaskStatus::Todo),
            None,
            None,
            1,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskCreated,
            &target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        let anchor_text = message_body_text(&transaction, message_id).await?;
        transaction.commit().await?;

        Ok(Task {
            message_id: message_id.to_owned(),
            target_id,
            number,
            status: TaskStatus::Todo,
            assignee_id: None,
            version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            anchor_text,
        })
    }

    /// Claim an unowned Task. A second actor receives a typed concurrency
    /// conflict; the current assignee may repeat the claim idempotently.
    pub async fn claim_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(&transaction, &current.target_id, actor_id, "claim task").await?;
        if current.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }
        if let Some(assignee_id) = &current.assignee_id {
            if assignee_id == actor_id {
                transaction.commit().await?;
                return Ok(current);
            }
            return Err(CollabError::TaskAlreadyClaimed {
                message_id: message_id.to_owned(),
                assignee_id: assignee_id.clone(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET assignee_id = ?2, status = 'in_progress', version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND assignee_id IS NULL AND version = ?5",
                (message_id, actor_id, next_version, now, current.version),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task claim compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "claimed",
            Some(current.status),
            Some(TaskStatus::InProgress),
            None,
            Some(actor_id),
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            message_id: message_id.to_owned(),
            target_id: current.target_id,
            number: current.number,
            status: TaskStatus::InProgress,
            assignee_id: Some(actor_id.to_owned()),
            version: next_version,
            created_at_ms: current.created_at_ms,
            updated_at_ms: now,
            anchor_text: current.anchor_text,
        })
    }

    /// Release a Task currently claimed by this actor while preserving its
    /// independent status. The expected version fences stale UI writes.
    pub async fn unclaim_task(
        &self,
        message_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Task> {
        self.assert_open()?;
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(
            &transaction,
            &current.target_id,
            actor_id,
            "unclaim task in",
        )
        .await?;
        if current.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: message_id.to_owned(),
                expected: expected_version,
                actual: current.version,
            });
        }
        let Some(assignee_id) = current.assignee_id.as_deref() else {
            transaction.commit().await?;
            return Ok(current);
        };
        if assignee_id != actor_id {
            return Err(CollabError::PermissionDenied {
                actor_id: actor_id.to_owned(),
                action: "unclaim another actor's task in",
                target_id: current.target_id,
            });
        }
        if current.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET assignee_id = NULL, version = ?2, updated_at_ms = ?3
                 WHERE message_id = ?1 AND assignee_id = ?4 AND version = ?5",
                (message_id, next_version, now, actor_id, current.version),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task unclaim compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "unclaimed",
            Some(current.status),
            Some(current.status),
            Some(actor_id),
            None,
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            assignee_id: None,
            version: next_version,
            updated_at_ms: now,
            ..current
        })
    }

    /// Move one Task through the explicit lifecycle with optimistic version
    /// fencing. Assignment remains unchanged.
    pub async fn update_task_status(
        &self,
        message_id: &str,
        actor_id: &str,
        status: TaskStatus,
        expected_version: i64,
    ) -> Result<Task> {
        self.assert_open()?;
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(&transaction, &current.target_id, actor_id, "update task in").await?;
        if current.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: message_id.to_owned(),
                expected: expected_version,
                actual: current.version,
            });
        }
        if current
            .assignee_id
            .as_deref()
            .is_some_and(|assignee_id| assignee_id != actor_id)
            && !is_owner(&transaction, &current.target_id, actor_id).await?
        {
            return Err(CollabError::PermissionDenied {
                actor_id: actor_id.to_owned(),
                action: "update another actor's task in",
                target_id: current.target_id,
            });
        }
        if current.status == status {
            transaction.commit().await?;
            return Ok(current);
        }
        if !task_transition_allowed(current.status, status) {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET status = ?2, version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND version = ?5",
                (
                    message_id,
                    status.as_str(),
                    next_version,
                    now,
                    current.version,
                ),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task status compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "status_changed",
            Some(current.status),
            Some(status),
            current.assignee_id.as_deref(),
            current.assignee_id.as_deref(),
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            status,
            version: next_version,
            updated_at_ms: now,
            ..current
        })
    }

    async fn create_actor(
        &self,
        kind: ActorKind,
        handle: &str,
        display_name: &str,
        workspace_path: Option<&str>,
    ) -> Result<Actor> {
        self.assert_open()?;
        require_non_empty("handle", handle)?;
        require_non_empty("display_name", display_name)?;
        let now = now_ms()?;
        let actor = Actor {
            id: new_id(),
            kind,
            handle: handle.to_owned(),
            display_name: display_name.to_owned(),
            created_at_ms: now,
        };
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    actor.id.as_str(),
                    actor.kind.as_str(),
                    actor.handle.as_str(),
                    actor.display_name.as_str(),
                    actor.created_at_ms,
                ),
            )
            .await?;
        if let Some(workspace_path) = workspace_path {
            let resolved_path = workspace_path.replace("{id}", actor.id.as_str());
            transaction
                .execute(
                    "INSERT INTO agents
                     (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, 'active', ?3, ?3)",
                    (actor.id.as_str(), resolved_path.as_str(), now),
                )
                .await?;
        }
        let actor_ids = all_actor_ids(&transaction).await?;
        insert_change(
            &transaction,
            ChangeKind::ActorCreated,
            None,
            &actor.id,
            &actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(actor)
    }

    async fn send_message_inner(
        &self,
        request: SendMessageRequest,
        _failpoint: SendFailpoint,
    ) -> Result<SendMessageResult> {
        self.assert_open()?;
        for (name, value) in [
            ("target_id", request.target_id.as_str()),
            ("author_id", request.author_id.as_str()),
            ("client_request_id", request.client_request_id.as_str()),
            ("text", request.text.as_str()),
        ] {
            require_non_empty(name, value)?;
        }

        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let route = require_target_access(
            &transaction,
            &request.target_id,
            &request.author_id,
            "send message to",
        )
        .await?;

        if let Some(message) =
            find_message_by_request(&transaction, &request.author_id, &request.client_request_id)
                .await?
        {
            let (recipient_ids, wake_agent_ids) =
                message_recipients(&transaction, &message.id).await?;
            transaction.commit().await?;
            return Ok(SendMessageResult {
                message,
                recipient_ids,
                wake_agent_ids,
                replayed: true,
            });
        }

        if route.kind == TargetKind::Thread {
            follow_thread_in_transaction(&transaction, &request.target_id, &request.author_id, now)
                .await?;
        }

        let message_id = new_id();
        let body_json = serde_json::to_string(&StoredTextBody {
            kind: "text".into(),
            text: request.text.clone(),
        })
        .map_err(|error| CollabError::InvalidArgument(error.to_string()))?;
        let mut rows = transaction
            .query(
                "INSERT INTO messages
                 (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 RETURNING seq",
                (
                    message_id.as_str(),
                    request.target_id.as_str(),
                    request.author_id.as_str(),
                    request.client_request_id.as_str(),
                    body_json.as_str(),
                    now,
                ),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::Database(
                "message insert returned no sequence".into(),
            ));
        };
        let message_seq = row.get::<i64>(0)?;
        drop(rows);

        #[cfg(test)]
        if _failpoint == SendFailpoint::AfterMessageInsert {
            return Err(CollabError::InjectedSendFailure);
        }

        let (recipient_statement, permission_target_id) = if route.kind == TargetKind::Thread {
            (
                "SELECT a.id, a.kind
                 FROM thread_follows f
                 JOIN actors a ON a.id = f.actor_id
                 JOIN memberships m
                   ON m.target_id = ?3 AND m.actor_id = f.actor_id
                 WHERE f.thread_target_id = ?1
                   AND f.unfollowed_at_ms IS NULL
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                route.permission_target_id(&request.target_id),
            )
        } else {
            (
                "SELECT a.id, a.kind
                 FROM memberships m
                 JOIN actors a ON a.id = m.actor_id
                 WHERE m.target_id = ?1
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                request.target_id.as_str(),
            )
        };
        let mut recipient_rows = if route.kind == TargetKind::Thread {
            transaction
                .query(
                    recipient_statement,
                    (
                        request.target_id.as_str(),
                        request.author_id.as_str(),
                        permission_target_id,
                    ),
                )
                .await?
        } else {
            transaction
                .query(
                    recipient_statement,
                    (request.target_id.as_str(), request.author_id.as_str()),
                )
                .await?
        };
        let mut recipients = Vec::new();
        while let Some(row) = recipient_rows.next().await? {
            recipients.push((row.get::<String>(0)?, row.get::<String>(1)?));
        }
        drop(recipient_rows);

        let mut recipient_ids = Vec::with_capacity(recipients.len());
        let mut wake_agent_ids = Vec::new();
        for (recipient_id, recipient_kind) in recipients {
            let delivery_id = new_id();
            transaction
                .execute(
                    "INSERT INTO deliveries
                     (id, message_id, message_seq, target_id, recipient_id, committed_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        delivery_id.as_str(),
                        message_id.as_str(),
                        message_seq,
                        request.target_id.as_str(),
                        recipient_id.as_str(),
                        now,
                    ),
                )
                .await?;
            if recipient_kind == ActorKind::Agent.as_str() {
                transaction
                    .execute(
                        "INSERT INTO agent_wake_state
                         (agent_id, pending_seq, notified_seq, notified_generation, attempt_count)
                         VALUES (?1, ?2, 0, 0, 0)
                         ON CONFLICT(agent_id) DO UPDATE SET
                           pending_seq = MAX(agent_wake_state.pending_seq, excluded.pending_seq)",
                        (recipient_id.as_str(), message_seq),
                    )
                    .await?;
                wake_agent_ids.push(recipient_id.clone());
            }
            recipient_ids.push(recipient_id);
        }
        insert_target_change(
            &transaction,
            ChangeKind::MessageCreated,
            &request.target_id,
            &message_id,
            &[request.author_id.as_str()],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(SendMessageResult {
            message: Message {
                seq: message_seq,
                id: message_id,
                target_id: request.target_id,
                author_id: request.author_id,
                client_request_id: request.client_request_id,
                text: request.text,
                created_at_ms: now,
            },
            recipient_ids,
            wake_agent_ids,
            replayed: false,
        })
    }

    fn assert_open(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            Err(CollabError::Closed)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredTextBody {
    kind: String,
    text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivityCursor {
    last_activity_seq: i64,
    conversation_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendFailpoint {
    None,
    #[cfg(test)]
    AfterMessageInsert,
}

async fn migrate(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(META_SCHEMA).await?;
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = 'schema_version'",
            (),
        )
        .await?;
    let stored = match rows.next().await? {
        Some(row) => Some(row.get::<String>(0)?),
        None => None,
    };
    drop(rows);
    let mut version = match stored {
        Some(found) => found
            .parse::<u32>()
            .map_err(|_| CollabError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            })?,
        None => 0,
    };
    if version > SCHEMA_VERSION {
        return Err(CollabError::SchemaVersionMismatch {
            found: version.to_string(),
            expected: SCHEMA_VERSION,
        });
    }

    while version < SCHEMA_VERSION {
        let next = version + 1;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        match next {
            1 => transaction.execute_batch(SCHEMA_V1).await?,
            2 => transaction.execute_batch(SCHEMA_V2).await?,
            3 => transaction.execute_batch(SCHEMA_V3).await?,
            4 => transaction.execute_batch(SCHEMA_V4).await?,
            5 => transaction.execute_batch(SCHEMA_V5).await?,
            _ => {
                return Err(CollabError::SchemaVersionMismatch {
                    found: version.to_string(),
                    expected: SCHEMA_VERSION,
                });
            }
        }
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES ('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [next.to_string()],
            )
            .await?;
        transaction.commit().await?;
        version = next;
    }
    Ok(())
}

async fn find_runtime_binding(
    connection: &Connection,
    key: &'static str,
    value: &str,
) -> Result<Option<RuntimeBinding>> {
    let statement = match key {
        "agent_id" => {
            "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
             FROM runtime_bindings WHERE agent_id = ?1"
        }
        "session_id" => {
            "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
             FROM runtime_bindings WHERE session_id = ?1"
        }
        _ => {
            return Err(CollabError::Database(
                "unsupported runtime binding lookup key".into(),
            ));
        }
    };
    let mut rows = connection.query(statement, [value]).await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(RuntimeBinding {
        agent_id: row.get(0)?,
        session_id: row.get(1)?,
        generation: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        preset: row.get(5)?,
        bound_at_ms: row.get(6)?,
    }))
}

fn message_from_row(row: &Row) -> Result<Message> {
    let body_json = row.get::<String>(5)?;
    let body: StoredTextBody = serde_json::from_str(&body_json)
        .map_err(|error| CollabError::Database(format!("message body is malformed: {error}")))?;
    Ok(Message {
        seq: row.get(0)?,
        id: row.get(1)?,
        target_id: row.get(2)?,
        author_id: row.get(3)?,
        client_request_id: row.get(4)?,
        text: body.text,
        created_at_ms: row.get(6)?,
    })
}

fn activity_inbox_item_from_row(row: &Row) -> Result<ActivityInboxItem> {
    let conversation_id = row.get::<String>(0)?;
    let target_kind_text = row.get::<String>(1)?;
    let target_kind = parse_target_kind(&conversation_id, &target_kind_text)?;
    let parent_target_id = row.get::<Option<String>>(2)?;
    let root_message_id = row.get::<Option<String>>(3)?;
    match target_kind {
        TargetKind::Thread if parent_target_id.is_none() || root_message_id.is_none() => {
            return Err(CollabError::Database(format!(
                "Activity Thread '{conversation_id}' is missing its parent or root Message"
            )));
        }
        TargetKind::Channel | TargetKind::Direct
            if parent_target_id.is_some() || root_message_id.is_some() =>
        {
            return Err(CollabError::Database(format!(
                "non-Thread Activity target '{conversation_id}' has Thread metadata"
            )));
        }
        _ => {}
    }
    let last_activity_seq = row.get::<i64>(4)?;
    if last_activity_seq <= 0 {
        return Err(CollabError::Database(format!(
            "Activity target '{conversation_id}' has invalid sequence {last_activity_seq}"
        )));
    }
    let title_body_json = row.get::<Option<String>>(6)?.ok_or_else(|| {
        CollabError::Database(format!(
            "Activity target '{conversation_id}' has no title Message"
        ))
    })?;
    let latest_kind_text = row.get::<String>(7)?;
    let latest_body_json = row.get::<String>(9)?;
    let reply_count = row.get::<Option<i64>>(11)?;
    if reply_count.is_some_and(|count| count < 0) {
        return Err(CollabError::Database(format!(
            "Activity target '{conversation_id}' has a negative reply count"
        )));
    }
    let task_number = row.get::<Option<i64>>(12)?;
    let task_status = row.get::<Option<String>>(13)?;
    let task = match (task_number, task_status) {
        (None, None) => None,
        (Some(number), Some(status_text)) if number > 0 => {
            let status = TaskStatus::parse(&status_text).ok_or_else(|| {
                CollabError::Database(format!(
                    "Activity target '{conversation_id}' has invalid Task status '{status_text}'"
                ))
            })?;
            Some(ActivityInboxTask {
                number,
                status,
                assignee_name: row.get(14)?,
            })
        }
        _ => {
            return Err(CollabError::Database(format!(
                "Activity target '{conversation_id}' has partial Task metadata"
            )));
        }
    };
    Ok(ActivityInboxItem {
        conversation_id: conversation_id.clone(),
        target_kind,
        parent_target_id,
        root_message_id,
        target_name: row.get(5)?,
        title_kind: if target_kind == TargetKind::Thread {
            ActivityTitleKind::Thread
        } else {
            ActivityTitleKind::Message
        },
        title: stored_text(
            &title_body_json,
            &format!("Activity title for '{conversation_id}'"),
        )?,
        latest_reply: Some(ActivityInboxReply {
            sender_name: row.get(8)?,
            sender_kind: parse_actor_kind(&conversation_id, &latest_kind_text)?,
            excerpt: stored_text(
                &latest_body_json,
                &format!("Activity preview for '{conversation_id}'"),
            )?,
            at_ms: row.get(10)?,
        }),
        last_activity_at_ms: row.get(10)?,
        last_activity_seq,
        reply_count,
        task,
    })
}

fn stored_text(body_json: &str, context: &str) -> Result<String> {
    serde_json::from_str::<StoredTextBody>(body_json)
        .map(|body| body.text)
        .map_err(|error| CollabError::Database(format!("{context} is malformed: {error}")))
}

fn parse_activity_cursor(value: &str) -> Result<ActivityCursor> {
    let Some((sequence, conversation_id)) = value.split_once(':') else {
        return Err(CollabError::InvalidArgument(
            "cursor must be '<sequence>:<target-id>'".into(),
        ));
    };
    let last_activity_seq = sequence.parse::<i64>().map_err(|_| {
        CollabError::InvalidArgument("cursor sequence must be a positive integer".into())
    })?;
    if last_activity_seq <= 0
        || conversation_id.is_empty()
        || conversation_id.contains(':')
        || Uuid::parse_str(conversation_id).is_err()
    {
        return Err(CollabError::InvalidArgument(
            "cursor must be '<positive-sequence>:<target-id>'".into(),
        ));
    }
    Ok(ActivityCursor {
        last_activity_seq,
        conversation_id: conversation_id.to_owned(),
    })
}

fn activity_cursor_for_item(item: &ActivityInboxItem) -> String {
    format!("{}:{}", item.last_activity_seq, item.conversation_id)
}

fn parse_actor_kind(actor_id: &str, value: &str) -> Result<ActorKind> {
    match value {
        "user" => Ok(ActorKind::User),
        "agent" => Ok(ActorKind::Agent),
        other => Err(CollabError::Database(format!(
            "actor '{actor_id}' has unknown kind '{other}'"
        ))),
    }
}

fn actor_from_row(row: &Row) -> Result<Actor> {
    let id = row.get::<String>(0)?;
    let kind_text = row.get::<String>(1)?;
    Ok(Actor {
        kind: parse_actor_kind(&id, &kind_text)?,
        id,
        handle: row.get(2)?,
        display_name: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

async fn find_actor(connection: &Connection, actor_id: &str) -> Result<Actor> {
    let mut rows = connection
        .query(
            "SELECT id, kind, handle, display_name, created_at_ms
             FROM actors WHERE id = ?1",
            [actor_id],
        )
        .await?;
    match rows.next().await? {
        Some(row) => actor_from_row(&row),
        None => Err(not_found("actor", actor_id)),
    }
}

async fn find_actor_by_handle(connection: &Connection, handle: &str) -> Result<Option<Actor>> {
    let mut rows = connection
        .query(
            "SELECT id, kind, handle, display_name, created_at_ms
             FROM actors WHERE handle = ?1",
            [handle],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| actor_from_row(&row))
        .transpose()
}

async fn require_actor(connection: &Connection, actor_id: &str) -> Result<ActorKind> {
    let mut rows = connection
        .query("SELECT kind FROM actors WHERE id = ?1", [actor_id])
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("actor", actor_id));
    };
    parse_actor_kind(actor_id, &row.get::<String>(0)?)
}

async fn require_agent(connection: &Connection, agent_id: &str) -> Result<()> {
    if require_actor(connection, agent_id).await? != ActorKind::Agent {
        return Err(not_found("agent", agent_id));
    }
    let mut rows = connection
        .query(
            "SELECT 1 FROM agents WHERE actor_id = ?1 AND lifecycle = 'active'",
            [agent_id],
        )
        .await?;
    if rows.next().await?.is_none() {
        return Err(not_found("active agent", agent_id));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct TargetRoute {
    kind: TargetKind,
    parent_target_id: Option<String>,
}

impl TargetRoute {
    fn permission_target_id<'a>(&'a self, exact_target_id: &'a str) -> &'a str {
        self.parent_target_id.as_deref().unwrap_or(exact_target_id)
    }
}

fn parse_target_kind(target_id: &str, value: &str) -> Result<TargetKind> {
    match value {
        "channel" => Ok(TargetKind::Channel),
        "direct" => Ok(TargetKind::Direct),
        "thread" => Ok(TargetKind::Thread),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' has unknown kind '{other}'"
        ))),
    }
}

async fn require_target_route(connection: &Connection, target_id: &str) -> Result<TargetRoute> {
    let mut rows = connection
        .query(
            "SELECT kind, parent_target_id
             FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
            [target_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("active target", target_id));
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
            return Err(not_found("active Thread parent target", parent_target_id));
        };
        let parent_kind_text = parent_row.get::<String>(0)?;
        let parent_kind = parse_target_kind(parent_target_id, &parent_kind_text)?;
        if parent_kind == TargetKind::Thread {
            return Err(CollabError::Database(format!(
                "Thread target '{target_id}' has a Thread parent"
            )));
        }
    }
    Ok(TargetRoute {
        kind,
        parent_target_id,
    })
}

async fn require_target(connection: &Connection, target_id: &str) -> Result<TargetKind> {
    Ok(require_target_route(connection, target_id).await?.kind)
}

async fn require_target_access(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
    action: &'static str,
) -> Result<TargetRoute> {
    let route = require_target_route(connection, target_id).await?;
    require_active_member(
        connection,
        route.permission_target_id(target_id),
        actor_id,
        action,
    )
    .await?;
    Ok(route)
}

async fn require_owner(connection: &Connection, target_id: &str, actor_id: &str) -> Result<()> {
    if is_owner(connection, target_id, actor_id).await? {
        return Ok(());
    }
    Err(CollabError::PermissionDenied {
        actor_id: actor_id.to_owned(),
        action: "manage",
        target_id: target_id.to_owned(),
    })
}

async fn is_owner(connection: &Connection, target_id: &str, actor_id: &str) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2
               AND role = 'owner' AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

async fn require_active_member(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
    action: &'static str,
) -> Result<()> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    if rows.next().await?.is_none() {
        return Err(CollabError::PermissionDenied {
            actor_id: actor_id.to_owned(),
            action,
            target_id: target_id.to_owned(),
        });
    }
    Ok(())
}

async fn is_active_member(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

async fn actor_handle(connection: &Connection, actor_id: &str) -> Result<String> {
    let mut rows = connection
        .query("SELECT handle FROM actors WHERE id = ?1", [actor_id])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Err(not_found("actor", actor_id)),
    }
}

async fn find_target(connection: &Connection, target_id: &str) -> Result<Target> {
    let mut rows = connection
        .query(
            "SELECT id, kind, name, parent_target_id, root_message_id,
                    created_by, created_at_ms
             FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
            [target_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("active target", target_id));
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

async fn targets_for_actor(connection: &Connection, actor_id: &str) -> Result<Vec<Target>> {
    let mut rows = connection
        .query(
            "SELECT DISTINCT target.id, target.kind, target.name,
                    target.parent_target_id, target.root_message_id,
                    target.created_by, target.created_at_ms
             FROM targets target
             JOIN memberships membership
               ON membership.target_id = CASE
                 WHEN target.kind = 'thread' THEN target.parent_target_id
                 ELSE target.id
               END
              AND membership.actor_id = ?1
              AND membership.left_at_ms IS NULL
             WHERE target.archived_at_ms IS NULL
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

async fn latest_change_seq(connection: &Connection) -> Result<i64> {
    let mut rows = connection
        .query("SELECT COALESCE(MAX(seq), 0) FROM change_events", ())
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::Database(
            "change sequence query returned no row".into(),
        ));
    };
    let latest = row.get::<i64>(0)?;
    drop(rows);
    Ok(latest.max(change_retention_floor(connection).await?))
}

async fn change_retention_floor(connection: &Connection) -> Result<i64> {
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = ?1",
            [CHANGE_RETENTION_FLOOR_KEY],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    let value = row.get::<String>(0)?;
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

fn change_from_row(row: &Row) -> Result<ChangeEvent> {
    let seq = row.get::<i64>(0)?;
    let kind_text = row.get::<String>(1)?;
    let Some(kind) = ChangeKind::parse(&kind_text) else {
        return Err(CollabError::Database(format!(
            "change '{seq}' has unknown kind '{kind_text}'"
        )));
    };
    Ok(ChangeEvent {
        seq,
        kind,
        target_id: row.get(2)?,
        entity_id: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

async fn follow_thread_in_transaction(
    connection: &Connection,
    thread_target_id: &str,
    actor_id: &str,
    now: i64,
) -> Result<bool> {
    let changed = connection
        .execute(
            "INSERT INTO thread_follows
             (thread_target_id, actor_id, followed_at_ms, unfollowed_at_ms)
             VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(thread_target_id, actor_id) DO UPDATE SET
               followed_at_ms = excluded.followed_at_ms,
               unfollowed_at_ms = NULL
             WHERE thread_follows.unfollowed_at_ms IS NOT NULL",
            (thread_target_id, actor_id, now),
        )
        .await?;
    Ok(changed == 1)
}

async fn is_following_thread(
    connection: &Connection,
    thread_target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM thread_follows
             WHERE thread_target_id = ?1
               AND actor_id = ?2
               AND unfollowed_at_ms IS NULL",
            (thread_target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

async fn followed_thread_ids_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT f.thread_target_id
             FROM thread_follows f
             JOIN targets t
               ON t.id = f.thread_target_id AND t.kind = 'thread'
             JOIN targets parent
               ON parent.id = t.parent_target_id
              AND parent.kind IN ('channel', 'direct')
             JOIN memberships m
               ON m.target_id = t.parent_target_id AND m.actor_id = f.actor_id
             WHERE f.actor_id = ?1
               AND f.unfollowed_at_ms IS NULL
               AND t.archived_at_ms IS NULL
               AND parent.archived_at_ms IS NULL
               AND m.left_at_ms IS NULL
             ORDER BY t.created_at_ms DESC, f.thread_target_id",
            [actor_id],
        )
        .await?;
    let mut target_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        target_ids.push(row.get(0)?);
    }
    Ok(target_ids)
}

async fn active_member_ids(connection: &Connection, target_id: &str) -> Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT actor_id FROM memberships
             WHERE target_id = ?1 AND left_at_ms IS NULL
             ORDER BY actor_id",
            [target_id],
        )
        .await?;
    let mut actor_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        actor_ids.push(row.get(0)?);
    }
    Ok(actor_ids)
}

async fn target_change_recipients(
    connection: &Connection,
    target_id: &str,
    extra_actor_ids: &[&str],
) -> Result<Vec<String>> {
    let route = require_target_route(connection, target_id).await?;
    let mut recipients = BTreeSet::new();
    // Change events drive authorized UI invalidation, not attention delivery.
    // A Thread therefore addresses every active parent member even when they
    // unfollow it; only Message Delivery/wake snapshots are follower-scoped.
    let mut rows = connection
        .query(
            "SELECT actor_id
             FROM memberships
             WHERE target_id = ?1 AND left_at_ms IS NULL",
            [route.permission_target_id(target_id)],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        recipients.insert(row.get::<String>(0)?);
    }
    drop(rows);
    recipients.extend(
        extra_actor_ids
            .iter()
            .map(|actor_id| (*actor_id).to_owned()),
    );
    Ok(recipients.into_iter().collect())
}

async fn all_actor_ids(connection: &Connection) -> Result<Vec<String>> {
    let mut rows = connection
        .query("SELECT id FROM actors ORDER BY id", ())
        .await?;
    let mut actor_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        actor_ids.push(row.get(0)?);
    }
    Ok(actor_ids)
}

async fn insert_change(
    connection: &Connection,
    kind: ChangeKind,
    target_id: Option<&str>,
    entity_id: &str,
    recipient_ids: &[String],
    now: i64,
) -> Result<i64> {
    let mut rows = connection
        .query(
            "INSERT INTO change_events
             (kind, target_id, entity_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             RETURNING seq",
            (kind.as_str(), target_id, entity_id, now),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::Database(
            "change insert returned no sequence".into(),
        ));
    };
    let seq = row.get::<i64>(0)?;
    drop(rows);
    for actor_id in recipient_ids {
        connection
            .execute(
                "INSERT INTO change_recipients (change_seq, actor_id)
                 VALUES (?1, ?2)",
                (seq, actor_id.as_str()),
            )
            .await?;
    }
    Ok(seq)
}

async fn insert_target_change(
    connection: &Connection,
    kind: ChangeKind,
    target_id: &str,
    entity_id: &str,
    extra_actor_ids: &[&str],
    now: i64,
) -> Result<i64> {
    let recipients = target_change_recipients(connection, target_id, extra_actor_ids).await?;
    insert_change(
        connection,
        kind,
        Some(target_id),
        entity_id,
        &recipients,
        now,
    )
    .await
}

async fn current_generation(connection: &Connection, agent_id: &str) -> Result<i64> {
    let mut rows = connection
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

async fn require_current_binding(
    connection: &Connection,
    agent_id: &str,
    generation: i64,
    session_id: &str,
) -> Result<()> {
    let mut rows = connection
        .query(
            "SELECT session_id, generation FROM runtime_bindings WHERE agent_id = ?1",
            [agent_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("runtime binding", agent_id));
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

async fn find_message_by_request(
    connection: &Connection,
    author_id: &str,
    client_request_id: &str,
) -> Result<Option<Message>> {
    let mut rows = connection
        .query(
            "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
             FROM messages WHERE author_id = ?1 AND client_request_id = ?2",
            (author_id, client_request_id),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let body_json = row.get::<String>(5)?;
    let body: StoredTextBody = serde_json::from_str(&body_json)
        .map_err(|error| CollabError::Database(format!("message body is malformed: {error}")))?;
    Ok(Some(Message {
        seq: row.get(0)?,
        id: row.get(1)?,
        target_id: row.get(2)?,
        author_id: row.get(3)?,
        client_request_id: row.get(4)?,
        text: body.text,
        created_at_ms: row.get(6)?,
    }))
}

async fn message_recipients(
    connection: &Connection,
    message_id: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut rows = connection
        .query(
            "SELECT d.recipient_id, a.kind
             FROM deliveries d JOIN actors a ON a.id = d.recipient_id
             WHERE d.message_id = ?1 ORDER BY d.recipient_id",
            [message_id],
        )
        .await?;
    let mut recipients = Vec::new();
    let mut agents = Vec::new();
    while let Some(row) = rows.next().await? {
        let recipient = row.get::<String>(0)?;
        if row.get::<String>(1)? == ActorKind::Agent.as_str() {
            agents.push(recipient.clone());
        }
        recipients.push(recipient);
    }
    Ok((recipients, agents))
}

async fn message_target(connection: &Connection, message_id: &str) -> Result<String> {
    let mut rows = connection
        .query("SELECT target_id FROM messages WHERE id = ?1", [message_id])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Err(not_found("message", message_id)),
    }
}

async fn message_body_text(connection: &Connection, message_id: &str) -> Result<Option<String>> {
    let mut rows = connection
        .query("SELECT body_json FROM messages WHERE id = ?1", [message_id])
        .await?;
    match rows.next().await? {
        Some(row) => {
            let body_json = row.get::<String>(0)?;
            let body: StoredTextBody = serde_json::from_str(&body_json).map_err(|error| {
                CollabError::Database(format!("message '{message_id}' has invalid body: {error}"))
            })?;
            Ok(Some(body.text))
        }
        None => Ok(None),
    }
}

async fn message_target_author(
    connection: &Connection,
    message_id: &str,
) -> Result<(String, String)> {
    let mut rows = connection
        .query(
            "SELECT target_id, author_id FROM messages WHERE id = ?1",
            [message_id],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok((row.get(0)?, row.get(1)?)),
        None => Err(not_found("message", message_id)),
    }
}

async fn find_task(connection: &Connection, message_id: &str) -> Result<Option<Task>> {
    let mut rows = connection
        .query(
            "SELECT task.message_id, task.target_id, task.number, task.status, task.assignee_id,
                    task.version, task.created_at_ms, task.updated_at_ms, message.body_json
             FROM tasks task
             LEFT JOIN messages message ON message.id = task.message_id
             WHERE task.message_id = ?1",
            [message_id],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| task_from_row(&row))
        .transpose()
}

fn task_from_row(row: &Row) -> Result<Task> {
    let message_id = row.get::<String>(0)?;
    let status_text = row.get::<String>(3)?;
    let status = TaskStatus::parse(&status_text).ok_or_else(|| {
        CollabError::Database(format!(
            "task '{message_id}' has invalid status '{status_text}'"
        ))
    })?;
    let anchor_text = row
        .get::<Option<String>>(8)?
        .map(|body_json| {
            serde_json::from_str::<StoredTextBody>(&body_json)
                .map(|body| body.text)
                .map_err(|error| {
                    CollabError::Database(format!(
                        "task '{message_id}' anchor has invalid body: {error}"
                    ))
                })
        })
        .transpose()?;
    Ok(Task {
        message_id,
        target_id: row.get(1)?,
        number: row.get(2)?,
        status,
        assignee_id: row.get(4)?,
        version: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        anchor_text,
    })
}

async fn tasks_for_actor(
    connection: &Connection,
    actor_id: &str,
    target_id: Option<&str>,
) -> Result<Vec<Task>> {
    let statement = if target_id.is_some() {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms,
                message.body_json
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
         LEFT JOIN messages message ON message.id = task.message_id
         WHERE task.target_id = ?2 AND target.archived_at_ms IS NULL
         ORDER BY task.number"
    } else {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms,
                message.body_json
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
         LEFT JOIN messages message ON message.id = task.message_id
         WHERE target.archived_at_ms IS NULL
         ORDER BY task.target_id, task.number"
    };
    let mut rows = if let Some(target_id) = target_id {
        connection.query(statement, (actor_id, target_id)).await?
    } else {
        connection.query(statement, [actor_id]).await?
    };
    let mut tasks = Vec::new();
    while let Some(row) = rows.next().await? {
        tasks.push(task_from_row(&row)?);
    }
    Ok(tasks)
}

fn task_transition_allowed(from: TaskStatus, to: TaskStatus) -> bool {
    matches!(
        (from, to),
        (TaskStatus::Todo, TaskStatus::InProgress)
            | (
                TaskStatus::InProgress,
                TaskStatus::Todo | TaskStatus::InReview
            )
            | (
                TaskStatus::InReview,
                TaskStatus::InProgress | TaskStatus::Done
            )
            | (TaskStatus::Done, TaskStatus::InProgress)
    )
}

#[allow(clippy::too_many_arguments)]
async fn insert_task_event(
    connection: &Connection,
    message_id: &str,
    actor_id: &str,
    event_type: &str,
    from_status: Option<TaskStatus>,
    to_status: Option<TaskStatus>,
    from_assignee_id: Option<&str>,
    to_assignee_id: Option<&str>,
    task_version: i64,
    created_at_ms: i64,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO task_events
             (message_id, actor_id, event_type, from_status, to_status,
              from_assignee_id, to_assignee_id, task_version, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                message_id,
                actor_id,
                event_type,
                from_status.map(TaskStatus::as_str),
                to_status.map(TaskStatus::as_str),
                from_assignee_id,
                to_assignee_id,
                task_version,
                created_at_ms,
            ),
        )
        .await?;
    Ok(())
}

fn require_non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CollabError::InvalidArgument(format!(
            "{name} must not be blank"
        )))
    } else {
        Ok(())
    }
}

fn not_found(entity: &'static str, id: &str) -> CollabError {
    CollabError::NotFound {
        entity,
        id: id.to_owned(),
    }
}

fn new_id() -> String {
    Uuid::now_v7().to_string()
}

fn now_ms() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CollabError::Filesystem(error.to_string()))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| CollabError::Filesystem("system clock is outside i64 milliseconds".into()))
}

fn prepare_database_parent(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    protect_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn protect_database_file(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") || !path.exists() {
        return Ok(());
    }
    protect_file(path)
}

#[cfg(unix)]
fn protect_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    async fn fixture() -> Result<(Arc<CollabCore>, Actor, Actor, Actor, Target)> {
        let core = Arc::new(CollabCore::open_memory().await?);
        let user = core.create_user("owner", "Owner").await?;
        let alpha = core.create_agent("alpha", "Alpha", "/tmp/alpha").await?;
        let beta = core.create_agent("beta", "Beta", "/tmp/beta").await?;
        let channel = core.create_channel("design", &user.id).await?;
        core.add_member(&channel.id, &alpha.id, &user.id).await?;
        core.add_member(&channel.id, &beta.id, &user.id).await?;
        Ok((core, user, alpha, beta, channel))
    }

    #[tokio::test]
    async fn send_commits_message_deliveries_and_wakes_together() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let result = core
            .send_message(SendMessageRequest {
                target_id: channel.id,
                author_id: user.id,
                client_request_id: "send-1".into(),
                text: "review this".into(),
            })
            .await?;

        assert_eq!(
            result.recipient_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert_eq!(
            result.wake_agent_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert!(!result.replayed);

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 1);
        assert_eq!(count(&connection, "deliveries").await?, 2);
        assert_eq!(count(&connection, "agent_wake_state").await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn send_rolls_back_message_when_recipient_phase_fails() -> Result<()> {
        let (core, user, _alpha, _beta, channel) = fixture().await?;
        let failure = core
            .send_message_inner(
                SendMessageRequest {
                    target_id: channel.id,
                    author_id: user.id,
                    client_request_id: "send-fail".into(),
                    text: "must roll back".into(),
                },
                SendFailpoint::AfterMessageInsert,
            )
            .await;
        assert!(matches!(failure, Err(CollabError::InjectedSendFailure)));

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 0);
        assert_eq!(count(&connection, "deliveries").await?, 0);
        assert_eq!(count(&connection, "agent_wake_state").await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn repeated_send_request_is_idempotent() -> Result<()> {
        let (core, user, _alpha, _beta, channel) = fixture().await?;
        let request = SendMessageRequest {
            target_id: channel.id,
            author_id: user.id,
            client_request_id: "same-request".into(),
            text: "only once".into(),
        };
        let first = core.send_message(request.clone()).await?;
        let replay = core.send_message(request).await?;
        assert_eq!(first.message, replay.message);
        assert!(replay.replayed);

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 1);
        assert_eq!(count(&connection, "deliveries").await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_generation_fences_model_seen_receipts() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let first_binding = core
            .bind_runtime(&alpha.id, "session-1", "openai", "codex", "default")
            .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: user.id,
            client_request_id: "generation-message".into(),
            text: "read me".into(),
        })
        .await?;
        let batch = core
            .check_inbox(
                &alpha.id,
                first_binding.generation,
                &first_binding.session_id,
                10,
            )
            .await?;
        let batch_id = batch.id.expect("batch with one message");
        assert_eq!(batch.messages.len(), 1);

        let second_binding = core
            .bind_runtime(&alpha.id, "session-2", "openai", "codex", "default")
            .await?;
        assert_eq!(second_binding.generation, first_binding.generation + 1);
        let stale = core
            .mark_model_seen(
                &batch_id,
                &alpha.id,
                first_binding.generation,
                &first_binding.session_id,
            )
            .await;
        assert!(matches!(
            stale,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));

        let redelivered = core
            .check_inbox(
                &alpha.id,
                second_binding.generation,
                &second_binding.session_id,
                10,
            )
            .await?;
        assert_eq!(redelivered.messages.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_lookup_keeps_session_ownership_unique() -> Result<()> {
        let (core, _user, alpha, beta, _channel) = fixture().await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-owned", "openai", "codex", "default")
            .await?;

        assert_eq!(
            core.runtime_binding(&alpha.id).await?,
            Some(binding.clone())
        );
        assert_eq!(
            core.runtime_binding_for_session("session-owned").await?,
            Some(binding)
        );
        assert!(
            core.bind_runtime(&beta.id, "session-owned", "openai", "codex", "default")
                .await
                .is_err()
        );
        assert_eq!(core.list_runtime_bindings().await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_preset_migration_preserves_session_generation() -> Result<()> {
        let (core, _user, alpha, _beta, _channel) = fixture().await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-legacy", "default", "default", "default")
            .await?;
        let migrated = core
            .update_runtime_preset(
                &alpha.id,
                binding.generation,
                &binding.session_id,
                "standard",
            )
            .await?;
        assert_eq!(migrated.session_id, binding.session_id);
        assert_eq!(migrated.generation, binding.generation);
        assert_eq!(migrated.bound_at_ms, binding.bound_at_ms);
        assert_eq!(migrated.preset, "standard");
        assert_eq!(core.runtime_binding(&alpha.id).await?, Some(migrated));

        let stale = core
            .update_runtime_preset(
                &alpha.id,
                binding.generation + 1,
                "session-legacy",
                "minimal",
            )
            .await;
        assert!(matches!(
            stale,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn wake_watermark_is_level_triggered_and_generation_fenced() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let first = core
            .bind_runtime(&alpha.id, "wake-session-1", "openai", "codex", "default")
            .await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "wake-message".into(),
                text: "ring once".into(),
            })
            .await?;

        let wakes = core.list_pending_wakes(10).await?;
        assert_eq!(wakes.len(), 1);
        assert_eq!(wakes[0].binding, first);
        assert_eq!(wakes[0].pending_seq, sent.message.seq);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        core.rearm_runtime_wake(&alpha.id, first.generation, &first.session_id)
            .await?;
        assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        {
            let connection = core.connection.lock().await;
            connection
                .execute(
                    "UPDATE memberships SET left_at_ms = 1
                     WHERE target_id = ?1 AND actor_id = ?2",
                    (channel.id.as_str(), alpha.id.as_str()),
                )
                .await?;
        }
        assert!(core.list_pending_wakes(10).await?.is_empty());
        core.add_member(&channel.id, &alpha.id, &user.id).await?;
        assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        let second = core
            .bind_runtime(&alpha.id, "wake-session-2", "openai", "codex", "default")
            .await?;
        let rebound = core.list_pending_wakes(10).await?;
        assert_eq!(rebound.len(), 1);
        assert_eq!(rebound[0].binding, second);
        assert!(matches!(
            core.mark_notified(
                &alpha.id,
                first.generation,
                &first.session_id,
                sent.message.seq,
            )
            .await,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));
        core.mark_notified(
            &alpha.id,
            second.generation,
            &second.session_id,
            sent.message.seq,
        )
        .await?;

        let batch = core
            .check_inbox(&alpha.id, second.generation, &second.session_id, 10)
            .await?;
        core.mark_model_seen(
            batch.id.as_deref().expect("one-message batch"),
            &alpha.id,
            second.generation,
            &second.session_id,
        )
        .await?;
        core.bind_runtime(&alpha.id, "wake-session-3", "openai", "codex", "default")
            .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn exact_target_reads_recheck_current_membership() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let outsider = core.create_user("outsider", "Outsider").await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "history-message".into(),
                text: "history".into(),
            })
            .await?;

        assert_eq!(
            core.read_message(&alpha.id, &channel.id, &sent.message.id)
                .await?,
            sent.message
        );
        assert_eq!(
            core.read_messages(&alpha.id, &channel.id, 0, 10).await?,
            vec![sent.message.clone()]
        );
        assert!(matches!(
            core.read_message(&outsider.id, &channel.id, &sent.message.id)
                .await,
            Err(CollabError::PermissionDenied { .. })
        ));

        let other = core.create_channel("other", &user.id).await?;
        core.add_member(&other.id, &alpha.id, &user.id).await?;
        assert!(matches!(
            core.read_message(&alpha.id, &other.id, &sent.message.id)
                .await,
            Err(CollabError::NotFound { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn read_messages_tail_returns_exact_count_and_true_tail() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let outsider = core.create_user("tail-outsider", "Tail Outsider").await?;
        let mut sent = Vec::new();
        for index in 0..5 {
            sent.push(
                core.send_message(SendMessageRequest {
                    target_id: channel.id.clone(),
                    author_id: user.id.clone(),
                    client_request_id: format!("tail-message-{index}"),
                    text: format!("tail {index}"),
                })
                .await?
                .message,
            );
        }

        // A small limit still reports the exact total and the true latest page.
        let tail = core.read_messages_tail(&alpha.id, &channel.id, 2).await?;
        assert_eq!(tail.count, 5);
        assert_eq!(tail.messages, sent[3..].to_vec());
        // A limit above the total returns everything, ascending.
        let full = core.read_messages_tail(&alpha.id, &channel.id, 100).await?;
        assert_eq!(full.count, 5);
        assert_eq!(full.messages, sent);
        // An empty target reports zero with no messages.
        let empty = core.create_channel("tail-empty", &user.id).await?;
        core.add_member(&empty.id, &alpha.id, &user.id).await?;
        let empty_tail = core.read_messages_tail(&alpha.id, &empty.id, 10).await?;
        assert_eq!(empty_tail.count, 0);
        assert!(empty_tail.messages.is_empty());
        // Membership and argument validation match the paged read.
        assert!(matches!(
            core.read_messages_tail(&outsider.id, &channel.id, 2).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.read_messages_tail(&alpha.id, &channel.id, 0).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.read_messages_tail(&alpha.id, &channel.id, 101).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn read_messages_tail_holds_one_snapshot_under_concurrent_writes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tail-snapshot.db");
        let reader = CollabCore::open(&path).await?;
        let user = reader.create_user("owner", "Owner").await?;
        let channel = reader.create_channel("busy", &user.id).await?;
        let writer = CollabCore::open(&path).await?;

        let writer_user = writer.ensure_user("owner", "Owner").await?;
        let write_target = channel.id.clone();

        let writer_task = tokio::spawn(async move {
            for index in 0..30 {
                writer
                    .send_message(SendMessageRequest {
                        target_id: write_target.clone(),
                        author_id: writer_user.id.clone(),
                        client_request_id: format!("concurrent-{index}"),
                        text: format!("concurrent {index}"),
                    })
                    .await?;
                tokio::task::yield_now().await;
            }
            Ok::<(), CollabError>(())
        });

        // Every read must be self-consistent: the page is the contiguous
        // suffix implied by the count of the same snapshot.
        for _ in 0..30 {
            let tail = reader.read_messages_tail(&user.id, &channel.id, 5).await?;
            let page_len = i64::try_from(tail.messages.len()).unwrap();
            assert!(page_len <= tail.count);
            if page_len > 0 {
                let first_seq = tail.messages[0].seq;
                assert_eq!(first_seq, tail.count - page_len + 1);
                for (offset, message) in tail.messages.iter().enumerate() {
                    assert_eq!(message.seq, first_seq + i64::try_from(offset).unwrap());
                }
            }
            tokio::task::yield_now().await;
        }
        writer_task.await.unwrap()?;
        Ok(())
    }

    #[tokio::test]
    async fn ensure_user_migrates_display_name_on_stable_handle() -> Result<()> {
        let core = CollabCore::open_memory().await?;
        let created = core.ensure_user("local-user", "Local User").await?;
        let channel = core.create_channel("identity", &created.id).await?;

        // Same handle with a new display name keeps the actor id, so
        // Memberships and Tasks survive the rename.
        let renamed = core.ensure_user("local-user", "Updated User").await?;
        assert_eq!(renamed.id, created.id);
        assert_eq!(renamed.display_name, "Updated User");
        let members = core.list_target_members(&renamed.id, &channel.id).await?;
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id, created.id);

        // Repeating with the current name is a no-op.
        let stable = core.ensure_user("local-user", "Updated User").await?;
        assert_eq!(stable, renamed);

        // A custom name is never overwritten by a later ensure.
        let custom = core.ensure_user("alice", "Custom Alice").await?;
        let kept = core.ensure_user("alice", "Updated User").await?;
        assert_eq!(kept.id, custom.id);
        assert_eq!(kept.display_name, "Custom Alice");
        Ok(())
    }

    #[tokio::test]
    async fn list_target_members_returns_only_active_channel_members() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let outsider = core.create_user("outsider", "Outsider").await?;

        // The projection is the Channel membership, not the actor directory:
        // the fixture created extra actors that never joined this channel.
        let members = core.list_target_members(&user.id, &channel.id).await?;
        let mut member_ids: Vec<&str> = members.iter().map(|actor| actor.id.as_str()).collect();
        member_ids.sort_unstable();
        let mut expected_ids = vec![alpha.id.as_str(), beta.id.as_str(), user.id.as_str()];
        expected_ids.sort_unstable();
        assert_eq!(member_ids, expected_ids);

        // A second channel has its own membership.
        let other = core.create_channel("other", &user.id).await?;
        let other_members = core.list_target_members(&user.id, &other.id).await?;
        assert_eq!(other_members.len(), 1);
        assert_eq!(other_members[0].id, user.id);

        // Non-members are rejected, and left members disappear.
        assert!(matches!(
            core.list_target_members(&outsider.id, &channel.id).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn task_reads_carry_authoritative_anchor_text() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let anchor = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "anchor-1".into(),
                text: "anchor body survives paging".into(),
            })
            .await?
            .message;
        let created = core.create_task(&anchor.id, &user.id).await?;
        assert_eq!(
            created.anchor_text.as_deref(),
            Some("anchor body survives paging")
        );

        // Push the anchor far outside any recent-message page; the Task read
        // still resolves the true anchor body from the store.
        for index in 0..120 {
            core.send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: format!("filler-{index}"),
                text: format!("filler {index}"),
            })
            .await?;
        }
        let tasks = core.list_tasks(&alpha.id, Some(&channel.id)).await?;
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].anchor_text.as_deref(),
            Some("anchor body survives paging")
        );

        // Mutations keep the anchor attached.
        let claimed = core.claim_task(&anchor.id, &alpha.id).await?;
        assert_eq!(
            claimed.anchor_text.as_deref(),
            Some("anchor body survives paging")
        );
        Ok(())
    }

    #[tokio::test]
    async fn direct_target_is_unique_for_an_unordered_actor_pair() -> Result<()> {
        let (core, user, alpha, beta, _channel) = fixture().await?;
        let direct = core.create_direct(&alpha.id, &beta.id).await?;
        let reversed = core.create_direct(&beta.id, &alpha.id).await?;
        assert_eq!(direct, reversed);
        assert_eq!(direct.kind, TargetKind::Direct);
        assert!(direct.parent_target_id.is_none());
        assert!(direct.root_message_id.is_none());

        let sent = core
            .send_message(SendMessageRequest {
                target_id: direct.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "direct-message".into(),
                text: "only beta receives this".into(),
            })
            .await?;
        assert_eq!(sent.recipient_ids, vec![beta.id.clone()]);
        assert_eq!(
            core.read_messages(&beta.id, &direct.id, 0, 10).await?,
            vec![sent.message]
        );
        assert!(matches!(
            core.read_messages(&user.id, &direct.id, 0, 10).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.add_member(&direct.id, &user.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn thread_inherits_parent_access_and_delivers_only_to_followers() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let outsider = core.create_user("thread-outsider", "Outsider").await?;
        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "thread-root".into(),
                text: "review in a thread".into(),
            })
            .await?;
        let beta_binding = core
            .bind_runtime(
                &beta.id,
                "thread-beta-session",
                "openai",
                "codex",
                "default",
            )
            .await?;
        let root_batch = core
            .check_inbox(
                &beta.id,
                beta_binding.generation,
                &beta_binding.session_id,
                10,
            )
            .await?;
        core.mark_model_seen(
            root_batch.id.as_deref().expect("root delivery batch"),
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
        )
        .await?;
        let beta_change_cursor = core.snapshot(&beta.id).await?.cursor;
        let thread = core.create_thread(&root.message.id, &alpha.id).await?;
        assert_eq!(thread.kind, TargetKind::Thread);
        assert_eq!(
            thread.parent_target_id.as_deref(),
            Some(channel.id.as_str())
        );
        assert_eq!(
            thread.root_message_id.as_deref(),
            Some(root.message.id.as_str())
        );
        assert_eq!(
            core.create_thread(&root.message.id, &beta.id).await?,
            thread
        );
        let beta_thread_changes = core.list_changes(&beta.id, beta_change_cursor, 50).await?;
        assert!(beta_thread_changes.iter().any(|change| {
            change.kind == ChangeKind::TargetCreated && change.entity_id == thread.id
        }));

        let first_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-1".into(),
                text: "alpha reply".into(),
            })
            .await?;
        assert_eq!(first_reply.recipient_ids, vec![user.id.clone()]);
        assert_eq!(
            core.snapshot(&alpha.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        assert!(
            core.snapshot(&beta.id)
                .await?
                .followed_thread_ids
                .is_empty()
        );

        core.follow_thread(&thread.id, &beta.id).await?;
        assert_eq!(
            core.snapshot(&beta.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        let second_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "thread-reply-2".into(),
                text: "owner reply".into(),
            })
            .await?;
        assert_eq!(
            second_reply.recipient_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert!(
            core.list_pending_wakes(10)
                .await?
                .iter()
                .any(|wake| wake.binding.agent_id == beta.id)
        );
        let thread_batch = core
            .check_inbox(
                &beta.id,
                beta_binding.generation,
                &beta_binding.session_id,
                10,
            )
            .await?;
        assert_eq!(thread_batch.messages.len(), 1);
        assert_eq!(thread_batch.messages[0].message.id, second_reply.message.id);
        core.mark_model_seen(
            thread_batch.id.as_deref().expect("Thread delivery batch"),
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
        )
        .await?;

        core.unfollow_thread(&thread.id, &beta.id).await?;
        assert!(
            core.snapshot(&beta.id)
                .await?
                .followed_thread_ids
                .is_empty()
        );
        let beta_unfollowed_cursor = core.snapshot(&beta.id).await?.cursor;
        assert_eq!(
            core.read_message(&beta.id, &thread.id, &second_reply.message.id)
                .await?,
            second_reply.message
        );
        let after_unfollow = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-3".into(),
                text: "beta should not receive this".into(),
            })
            .await?;
        assert_eq!(after_unfollow.recipient_ids, vec![user.id.clone()]);
        let beta_realtime_changes = core
            .list_changes(&beta.id, beta_unfollowed_cursor, 50)
            .await?;
        assert!(beta_realtime_changes.iter().any(|change| {
            change.kind == ChangeKind::MessageCreated
                && change.entity_id == after_unfollow.message.id
        }));

        let beta_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: beta.id.clone(),
                client_request_id: "thread-reply-4".into(),
                text: "participating follows again".into(),
            })
            .await?;
        let mut expected_beta_reply_recipients = vec![user.id.clone(), alpha.id.clone()];
        expected_beta_reply_recipients.sort();
        assert_eq!(beta_reply.recipient_ids, expected_beta_reply_recipients);
        let final_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-5".into(),
                text: "beta follows again".into(),
            })
            .await?;
        assert!(final_reply.recipient_ids.contains(&beta.id));
        assert_eq!(
            core.snapshot(&beta.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        assert!(matches!(
            core.read_messages(&outsider.id, &thread.id, 0, 10).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.create_task(&first_reply.message.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.create_thread(&first_reply.message.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_and_change_cursor_are_authorization_filtered() -> Result<()> {
        let core = CollabCore::open_memory().await?;
        let owner = core.ensure_user("cursor-owner", "Owner").await?;
        // A repeated ensure with the current name is a no-op returning the
        // same actor; a different name is an explicit migration (covered by
        // ensure_user_migrates_display_name_on_stable_handle).
        assert_eq!(core.ensure_user("cursor-owner", "Owner").await?, owner);
        let alpha = core
            .create_agent("cursor-alpha", "Alpha", "/tmp/cursor-alpha")
            .await?;
        let outsider = core.ensure_user("cursor-outsider", "Outsider").await?;
        let channel = core.create_channel("cursor-channel", &owner.id).await?;
        core.add_member(&channel.id, &owner.id, &owner.id).await?;
        core.add_member(&channel.id, &alpha.id, &owner.id).await?;

        let owner_snapshot = core.snapshot(&owner.id).await?;
        assert_eq!(owner_snapshot.actor, owner);
        assert_eq!(owner_snapshot.targets, vec![channel.clone()]);
        assert!(owner_snapshot.followed_thread_ids.is_empty());
        assert!(owner_snapshot.tasks.is_empty());
        assert!(core.snapshot(&outsider.id).await?.targets.is_empty());

        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: owner.id.clone(),
                client_request_id: "cursor-message".into(),
                text: "durable change".into(),
            })
            .await?;
        let owner_changes = core
            .list_changes(&owner.id, owner_snapshot.cursor, 50)
            .await?;
        assert_eq!(owner_changes.len(), 1);
        assert_eq!(owner_changes[0].kind, ChangeKind::MessageCreated);
        assert_eq!(
            owner_changes[0].target_id.as_deref(),
            Some(channel.id.as_str())
        );
        assert_eq!(owner_changes[0].entity_id, sent.message.id);
        assert_eq!(
            core.list_changes(&alpha.id, owner_snapshot.cursor, 50)
                .await?,
            owner_changes
        );
        assert!(
            core.list_changes(&outsider.id, owner_snapshot.cursor, 50)
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[tokio::test]
    async fn change_retention_requires_snapshot_resync_outside_retained_range() -> Result<()> {
        let (core, user, _alpha, _beta, channel) = fixture().await?;
        let cursor = core.snapshot(&user.id).await?.cursor;
        assert!(cursor > 0);

        let floor = core.prune_changes_before(now_ms()? + 1).await?;
        assert_eq!(floor, cursor);
        assert_eq!(core.snapshot(&user.id).await?.cursor, floor);
        assert!(core.list_changes(&user.id, floor, 10).await?.is_empty());
        for after_seq in [floor - 1, floor + 1] {
            assert!(matches!(
                core.list_changes(&user.id, after_seq, 10).await,
                Err(CollabError::ChangeCursorOutOfRange {
                    minimum_cursor,
                    maximum_cursor,
                    ..
                }) if minimum_cursor == floor && maximum_cursor == floor
            ));
        }

        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id,
                author_id: user.id.clone(),
                client_request_id: "after-retention".into(),
                text: "new retained change".into(),
            })
            .await?;
        let changes = core.list_changes(&user.id, floor, 10).await?;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, sent.message.id);
        assert_eq!(core.prune_changes_before(0).await?, floor);
        Ok(())
    }

    #[tokio::test]
    async fn task_lifecycle_uses_version_fencing_and_emits_changes() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let before = core.snapshot(&user.id).await?.cursor;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "task-lifecycle-message".into(),
                text: "finish the lifecycle".into(),
            })
            .await?;
        let created = core.create_task(&sent.message.id, &user.id).await?;
        let claimed = core.claim_task(&sent.message.id, &alpha.id).await?;
        assert_eq!(claimed.version, created.version + 1);
        assert!(matches!(
            core.update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::InReview,
                claimed.version,
            )
            .await,
            Err(CollabError::PermissionDenied { .. })
        ));

        let owner_review = core
            .update_task_status(
                &sent.message.id,
                &user.id,
                TaskStatus::InReview,
                claimed.version,
            )
            .await?;
        let reopened = core
            .update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::InProgress,
                owner_review.version,
            )
            .await?;
        let review = core
            .update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::InReview,
                reopened.version,
            )
            .await?;
        assert!(matches!(
            core.update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::Done,
                claimed.version,
            )
            .await,
            Err(CollabError::TaskVersionConflict { .. })
        ));
        let unclaimed = core
            .unclaim_task(&sent.message.id, &alpha.id, review.version)
            .await?;
        assert_eq!(unclaimed.status, TaskStatus::InReview);
        assert!(unclaimed.assignee_id.is_none());

        let beta_claim = core.claim_task(&sent.message.id, &beta.id).await?;
        let beta_review = core
            .update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::InReview,
                beta_claim.version,
            )
            .await?;
        let done = core
            .update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::Done,
                beta_review.version,
            )
            .await?;
        assert!(matches!(
            core.unclaim_task(&sent.message.id, &beta.id, done.version)
                .await,
            Err(CollabError::TaskTransitionDenied { .. })
        ));
        assert_eq!(
            core.list_tasks(&user.id, Some(&channel.id)).await?,
            vec![done]
        );
        let changes = core.list_changes(&user.id, before, 50).await?;
        assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == ChangeKind::TaskCreated)
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == ChangeKind::TaskUpdated)
                .count(),
            8
        );
        Ok(())
    }

    #[tokio::test]
    async fn only_one_concurrent_task_claim_wins() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id,
                author_id: user.id.clone(),
                client_request_id: "task-message".into(),
                text: "implement it".into(),
            })
            .await?;
        core.create_task(&sent.message.id, &user.id).await?;

        let alpha_claim = core.claim_task(&sent.message.id, &alpha.id);
        let beta_claim = core.claim_task(&sent.message.id, &beta.id);
        let (alpha_result, beta_result) = tokio::join!(alpha_claim, beta_claim);
        let successes = usize::from(alpha_result.is_ok()) + usize::from(beta_result.is_ok());
        let conflicts = usize::from(matches!(
            alpha_result,
            Err(CollabError::TaskAlreadyClaimed { .. })
        )) + usize::from(matches!(
            beta_result,
            Err(CollabError::TaskAlreadyClaimed { .. })
        ));
        assert_eq!(successes, 1);
        assert_eq!(conflicts, 1);
        Ok(())
    }

    #[tokio::test]
    async fn activity_inbox_projects_done_revive_direct_and_task_metadata() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let task_channel = core.create_channel("tasks", &user.id).await?;
        core.add_member(&task_channel.id, &alpha.id, &user.id)
            .await?;
        let task_message = core
            .send_message(SendMessageRequest {
                target_id: task_channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "activity-task".into(),
                text: "ship the inbox".into(),
            })
            .await?;
        core.create_task(&task_message.message.id, &user.id).await?;
        core.claim_task(&task_message.message.id, &alpha.id).await?;

        let direct = core.create_direct(&user.id, &alpha.id).await?;
        core.send_message(SendMessageRequest {
            target_id: direct.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "activity-direct".into(),
            text: "direct update".into(),
        })
        .await?;

        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "activity-root".into(),
                text: "thread root".into(),
            })
            .await?;
        core.create_task(&root.message.id, &user.id).await?;
        core.claim_task(&root.message.id, &alpha.id).await?;
        let thread = core.create_thread(&root.message.id, &beta.id).await?;
        let reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: beta.id.clone(),
                client_request_id: "activity-reply".into(),
                text: "thread reply".into(),
            })
            .await?;

        let page = core.inbox_list(&user.id, 20, None).await?;
        assert_eq!(page.active_count, 4);
        assert_eq!(page.items.len(), 4);

        let direct_item = page
            .items
            .iter()
            .find(|item| item.conversation_id == direct.id)
            .expect("Direct Activity item");
        assert_eq!(direct_item.target_name, alpha.display_name);
        assert_eq!(direct_item.title, "direct update");
        assert_eq!(
            direct_item
                .latest_reply
                .as_ref()
                .map(|reply| reply.sender_kind),
            Some(ActorKind::Agent)
        );
        assert_eq!(direct_item.reply_count, None);

        let task_item = page
            .items
            .iter()
            .find(|item| item.conversation_id == task_channel.id)
            .expect("Channel Task Activity item");
        let task_badge = task_item.task.as_ref().expect("Channel Task badge");
        assert_eq!(task_badge.number, 1);
        assert_eq!(task_badge.status, TaskStatus::InProgress);
        assert_eq!(task_badge.assignee_name.as_deref(), Some("Alpha"));

        let thread_item = page
            .items
            .iter()
            .find(|item| item.conversation_id == thread.id)
            .expect("Thread Activity item");
        assert_eq!(
            thread_item.parent_target_id.as_deref(),
            Some(channel.id.as_str())
        );
        assert_eq!(
            thread_item.root_message_id.as_deref(),
            Some(root.message.id.as_str())
        );
        assert_eq!(thread_item.title_kind, ActivityTitleKind::Thread);
        assert_eq!(thread_item.title, "thread root");
        assert_eq!(thread_item.reply_count, Some(1));
        assert_eq!(thread_item.last_activity_seq, reply.message.seq);
        assert_eq!(
            thread_item
                .latest_reply
                .as_ref()
                .map(|preview| preview.excerpt.as_str()),
            Some("thread reply")
        );
        assert_eq!(
            thread_item.task.as_ref().map(|task| task.status),
            Some(TaskStatus::InProgress)
        );

        let cursor_before_done = core.snapshot(&user.id).await?.cursor;
        let alpha_cursor_before_done = core.snapshot(&alpha.id).await?.cursor;
        core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
            .await?;
        let after_done = core.inbox_list(&user.id, 20, None).await?;
        assert_eq!(after_done.active_count, 3);
        assert!(
            after_done
                .items
                .iter()
                .all(|item| item.conversation_id != task_channel.id)
        );
        let done_changes = core.list_changes(&user.id, cursor_before_done, 10).await?;
        assert_eq!(done_changes.len(), 1);
        assert_eq!(done_changes[0].kind, ChangeKind::ActivityDoneChanged);
        assert!(
            core.list_changes(&alpha.id, alpha_cursor_before_done, 10)
                .await?
                .is_empty()
        );

        let cursor_after_done = done_changes[0].seq;
        core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
            .await?;
        assert!(
            core.list_changes(&user.id, cursor_after_done, 10)
                .await?
                .is_empty()
        );

        let reopened = core
            .send_message(SendMessageRequest {
                target_id: task_channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "activity-reopen".into(),
                text: "new activity".into(),
            })
            .await?;
        core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
            .await?;
        let after_reopen = core.inbox_list(&user.id, 20, None).await?;
        assert_eq!(after_reopen.active_count, 4);
        assert_eq!(
            after_reopen
                .items
                .iter()
                .find(|item| item.conversation_id == task_channel.id)
                .map(|item| item.last_activity_seq),
            Some(reopened.message.seq)
        );
        Ok(())
    }

    #[tokio::test]
    async fn activity_inbox_enforces_membership_and_thread_follow_scope() -> Result<()> {
        let (core, user, _alpha, beta, channel) = fixture().await?;
        let outsider = core.create_user("outsider", "Outsider").await?;
        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "follow-root".into(),
                text: "follow scope".into(),
            })
            .await?;
        let thread = core.create_thread(&root.message.id, &beta.id).await?;
        let reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: beta.id.clone(),
                client_request_id: "follow-reply".into(),
                text: "followed reply".into(),
            })
            .await?;

        assert!(
            core.inbox_list(&user.id, 20, None)
                .await?
                .items
                .iter()
                .any(|item| item.conversation_id == thread.id)
        );
        core.unfollow_thread(&thread.id, &user.id).await?;
        assert!(
            core.inbox_list(&user.id, 20, None)
                .await?
                .items
                .iter()
                .all(|item| item.conversation_id != thread.id)
        );
        assert!(matches!(
            core.inbox_done(&user.id, &thread.id, reply.message.seq)
                .await,
            Err(CollabError::NotFound { .. })
        ));
        core.follow_thread(&thread.id, &user.id).await?;
        assert!(
            core.inbox_list(&user.id, 20, None)
                .await?
                .items
                .iter()
                .any(|item| item.conversation_id == thread.id)
        );

        assert!(
            core.inbox_list(&outsider.id, 20, None)
                .await?
                .items
                .is_empty()
        );
        assert!(matches!(
            core.inbox_done(&outsider.id, &channel.id, root.message.seq)
                .await,
            Err(CollabError::PermissionDenied { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn activity_inbox_cursor_pages_without_duplicates_or_skips() -> Result<()> {
        let core = CollabCore::open_memory().await?;
        let user = core.create_user("pager", "Pager").await?;
        let mut expected = Vec::new();
        for index in 0..5 {
            let channel = core
                .create_channel(&format!("page-{index}"), &user.id)
                .await?;
            core.send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: format!("page-message-{index}"),
                text: format!("activity {index}"),
            })
            .await?;
            expected.push(channel.id);
        }
        expected.reverse();

        let first = core.inbox_list(&user.id, 2, None).await?;
        assert_eq!(first.active_count, 5);
        assert_eq!(first.items.len(), 2);
        let second = core
            .inbox_list(&user.id, 2, first.next_cursor.as_deref())
            .await?;
        assert_eq!(second.items.len(), 2);
        let third = core
            .inbox_list(&user.id, 2, second.next_cursor.as_deref())
            .await?;
        assert_eq!(third.items.len(), 1);
        assert!(third.next_cursor.is_none());
        let actual = first
            .items
            .iter()
            .chain(&second.items)
            .chain(&third.items)
            .map(|item| item.conversation_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        let mut unique = actual.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), actual.len());

        assert!(matches!(
            core.inbox_list(&user.id, 0, None).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.inbox_list(&user.id, 20, Some("bad-cursor")).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.inbox_done(&user.id, &expected[0], i64::MAX).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn schema_v4_upgrades_change_ledger_for_activity_done() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("v4.db");
        let user_id = "018f0000-0000-7000-8000-000000000001";
        let target_id = "018f0000-0000-7000-8000-000000000002";
        {
            let database =
                turso::Builder::new_local(path.to_str().ok_or_else(|| {
                    CollabError::Filesystem("temporary path is not UTF-8".into())
                })?)
                .build()
                .await?;
            let mut connection = database.connect()?;
            connection.execute_batch(META_SCHEMA).await?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute_batch(SCHEMA_V1).await?;
            transaction.execute_batch(SCHEMA_V2).await?;
            transaction.execute_batch(SCHEMA_V3).await?;
            transaction.execute_batch(SCHEMA_V4).await?;
            transaction
                .execute(
                    "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                     VALUES (?1, 'user', 'legacy-owner', 'Legacy Owner', 1)",
                    [user_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO targets
                     (id, kind, name, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, 'channel', 'legacy', ?2, 1, NULL)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, 'owner', 1, NULL)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO messages
                     (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                     VALUES ('018f0000-0000-7000-8000-000000000003', ?1, ?2,
                             'legacy-request', '{\"kind\":\"text\",\"text\":\"legacy\"}', 1)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO change_events
                     (kind, target_id, entity_id, created_at_ms)
                     VALUES ('message_created', ?1,
                             '018f0000-0000-7000-8000-000000000003', 1)",
                    [target_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO change_recipients (change_seq, actor_id) VALUES (1, ?1)",
                    [user_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '4')",
                    (),
                )
                .await?;
            transaction.commit().await?;
        }

        let core = CollabCore::open(&path).await?;
        let page = core.inbox_list(user_id, 20, None).await?;
        assert_eq!(page.active_count, 1);
        let through_seq = page.items[0].last_activity_seq;
        core.inbox_done(user_id, target_id, through_seq).await?;
        let changes = core.list_changes(user_id, 0, 10).await?;
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
        assert_eq!(changes[1].kind, ChangeKind::ActivityDoneChanged);
        Ok(())
    }

    #[tokio::test]
    async fn schema_v1_file_upgrades_to_unique_session_bindings() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("v1.db");
        {
            let database =
                turso::Builder::new_local(path.to_str().ok_or_else(|| {
                    CollabError::Filesystem("temporary path is not UTF-8".into())
                })?)
                .build()
                .await?;
            let mut connection = database.connect()?;
            connection.execute_batch(META_SCHEMA).await?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute_batch(SCHEMA_V1).await?;
            transaction
                .execute(
                    "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '1')",
                    (),
                )
                .await?;
            transaction.commit().await?;
        }

        let core = CollabCore::open(&path).await?;
        let alpha = core
            .create_agent("v1-alpha", "Alpha", "/tmp/v1-alpha")
            .await?;
        let beta = core.create_agent("v1-beta", "Beta", "/tmp/v1-beta").await?;
        core.bind_runtime(&alpha.id, "unique-session", "openai", "codex", "default")
            .await?;
        assert!(
            core.bind_runtime(&beta.id, "unique-session", "openai", "codex", "default")
                .await
                .is_err()
        );
        let connection = core.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT value FROM collab_meta WHERE key = 'schema_version'",
                (),
            )
            .await?;
        assert_eq!(
            rows.next().await?.expect("schema row").get::<String>(0)?,
            SCHEMA_VERSION.to_string()
        );
        Ok(())
    }

    #[tokio::test]
    async fn local_turso_file_reopens_with_agent_identity_intact() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("state.db");
        let owner_id;
        {
            let core = CollabCore::open(&path).await?;
            owner_id = core.create_user("persistent-owner", "Owner").await?.id;
            core.close().await?;
        }

        let reopened = CollabCore::open(&path).await?;
        let channel = reopened.create_channel("after-restart", &owner_id).await?;
        assert_eq!(channel.created_by, owner_id);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)?.permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(directory.path())?.permissions().mode() & 0o777,
                0o700
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn delete_agent_removes_operational_state_but_keeps_history() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        core.create_direct(&user.id, &alpha.id).await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-delete", "openai", "codex", "default")
            .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "wake-alpha".into(),
            text: "wake up".into(),
        })
        .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "alpha-message".into(),
            text: "alpha was here".into(),
        })
        .await?;
        let batch = core
            .check_inbox(&alpha.id, binding.generation, &binding.session_id, 10)
            .await?;
        assert!(batch.id.is_some());
        let directory_before = core.list_actors(&user.id).await?;
        assert!(directory_before.iter().any(|actor| actor.id == alpha.id));
        let actors_before = {
            let connection = core.connection.lock().await;
            count(&connection, "actors").await?
        };

        core.delete_agent(&alpha.id).await?;

        // The deleted Agent leaves the directory but keeps its actors row and
        // stays readable as the author of its history messages.
        let directory_after = core.list_actors(&user.id).await?;
        assert!(!directory_after.iter().any(|actor| actor.id == alpha.id));
        assert!(directory_after.iter().any(|actor| actor.id == user.id));
        let history = core.read_messages(&user.id, &channel.id, 0, 10).await?;
        assert!(
            history
                .iter()
                .any(|message| message.author_id == alpha.id
                    && message.text == "alpha was here")
        );

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "actors").await?, actors_before);
        assert_eq!(
            count_where(&connection, "memberships", "actor_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "runtime_bindings", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "agents", "actor_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "agent_wake_state", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "inbox_batches", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(count(&connection, "inbox_batch_items").await?, 0);
        assert_eq!(
            count_where(&connection, "actors", "id", &alpha.id).await?,
            1
        );
        assert_eq!(
            count_where(&connection, "messages", "author_id", &alpha.id).await?,
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_agent_rejects_users_and_missing_actors() -> Result<()> {
        let (core, user, _alpha, _beta, _channel) = fixture().await?;
        assert!(matches!(
            core.delete_agent(&user.id).await,
            Err(CollabError::NotFound { entity: "agent", .. })
        ));
        assert!(matches!(
            core.delete_agent("missing-actor").await,
            Err(CollabError::NotFound { entity: "actor", .. })
        ));
        Ok(())
    }

    async fn count(connection: &Connection, table: &str) -> Result<i64> {
        let allowed = [
            "messages",
            "deliveries",
            "agent_wake_state",
            "inbox_batch_items",
            "actors",
        ];
        assert!(allowed.contains(&table));
        let mut rows = connection
            .query(format!("SELECT COUNT(*) FROM {table}"), ())
            .await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
        Ok(row.get(0)?)
    }

    async fn count_where(
        connection: &Connection,
        table: &str,
        column: &str,
        value: &str,
    ) -> Result<i64> {
        let allowed = [
            ("memberships", "actor_id"),
            ("runtime_bindings", "agent_id"),
            ("agents", "actor_id"),
            ("agent_wake_state", "agent_id"),
            ("inbox_batches", "agent_id"),
            ("actors", "id"),
            ("messages", "author_id"),
        ];
        assert!(allowed.contains(&(table, column)));
        let mut rows = connection
            .query(
                format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                [value],
            )
            .await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
        Ok(row.get(0)?)
    }
}
