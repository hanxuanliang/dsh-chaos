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
    Actor, ActorKind, ChangeEvent, ChangeKind, CollabSnapshot, InboxBatch, InboxMessage, Message,
    PendingWake, RuntimeBinding, SendMessageRequest, SendMessageResult, Target, TargetKind, Task,
    TaskStatus,
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

use crate::schema::{META_SCHEMA, SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_VERSION};

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

    /// List the local actor directory after authenticating the caller.
    pub async fn list_actors(&self, actor_id: &str) -> Result<Vec<Actor>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        let mut rows = connection
            .query(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors ORDER BY handle, id",
                (),
            )
            .await?;
        let mut actors = Vec::new();
        while let Some(row) = rows.next().await? {
            actors.push(actor_from_row(&row)?);
        }
        Ok(actors)
    }

    /// Return one authorization-filtered bootstrap projection and the global
    /// durable change cursor observed in the same connection critical section.
    pub async fn snapshot(&self, actor_id: &str) -> Result<CollabSnapshot> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        let actor = find_actor(&connection, actor_id).await?;
        let cursor = latest_change_seq(&connection).await?;
        let targets = targets_for_actor(&connection, actor_id).await?;
        let tasks = tasks_for_actor(&connection, actor_id, None).await?;
        Ok(CollabSnapshot {
            actor,
            cursor,
            targets,
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
            transaction
                .execute(
                    "INSERT INTO agents
                     (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, 'active', ?3, ?3)",
                    (actor.id.as_str(), workspace_path, now),
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
    Ok(row.get(0)?)
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
    let statement = if route.kind == TargetKind::Thread {
        "SELECT f.actor_id
         FROM thread_follows f
         JOIN memberships m
           ON m.target_id = ?2 AND m.actor_id = f.actor_id
         WHERE f.thread_target_id = ?1
           AND f.unfollowed_at_ms IS NULL
           AND m.left_at_ms IS NULL"
    } else {
        "SELECT actor_id
         FROM memberships
         WHERE target_id = ?1 AND left_at_ms IS NULL"
    };
    let mut rows = if route.kind == TargetKind::Thread {
        connection
            .query(
                statement,
                (target_id, route.permission_target_id(target_id)),
            )
            .await?
    } else {
        connection.query(statement, [target_id]).await?
    };
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
            "SELECT message_id, target_id, number, status, assignee_id,
                    version, created_at_ms, updated_at_ms
             FROM tasks WHERE message_id = ?1",
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
    Ok(Task {
        message_id,
        target_id: row.get(1)?,
        number: row.get(2)?,
        status,
        assignee_id: row.get(4)?,
        version: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
    })
}

async fn tasks_for_actor(
    connection: &Connection,
    actor_id: &str,
    target_id: Option<&str>,
) -> Result<Vec<Task>> {
    let statement = if target_id.is_some() {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
         WHERE task.target_id = ?2 AND target.archived_at_ms IS NULL
         ORDER BY task.number"
    } else {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
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

        core.follow_thread(&thread.id, &beta.id).await?;
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
        assert_eq!(core.ensure_user("cursor-owner", "Ignored").await?, owner);
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

    async fn count(connection: &Connection, table: &str) -> Result<i64> {
        let allowed = ["messages", "deliveries", "agent_wake_state"];
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
}
