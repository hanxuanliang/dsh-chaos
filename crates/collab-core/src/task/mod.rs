//! Task projection, claim ownership, and lifecycle transitions.

mod model;
pub(crate) mod store;

pub use model::{Task, TaskStatus};

use crate::actor::ActorId;
use crate::changefeed::insert_target_change;
use crate::membership::Membership;
use crate::message::store::MessageStore;
use crate::target::require_target_access;
use crate::{Actor, ChangeKind, CollabCore, CollabError, Result, TargetKind, now_ms};

use model::{NewTask, TaskEvent, TaskEventKind, task_transition_allowed};
use store::TaskStore;

impl CollabCore {
    /// Convert a committed top-level Message to a Task with a target-local
    /// monotonic task number.
    pub async fn create_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = TaskStore::new(connection);
            let target_id = MessageStore::new(connection).target_of(message_id).await?;
            let route = require_target_access(connection, &target_id, actor_id).await?;
            if route.kind == TargetKind::Thread {
                return Err(CollabError::InvalidArgument(
                    "Thread replies cannot become Tasks".into(),
                ));
            }

            if let Some(task) = store.find_by_message(message_id).await? {
                return Ok(task);
            }

            let number = store.next_number(&target_id).await?;
            store
                .insert(&NewTask {
                    message_id,
                    target_id: target_id.as_str(),
                    number,
                    created_at_ms: now,
                })
                .await?;
            store
                .record_event(&TaskEvent {
                    message_id,
                    actor_id,
                    kind: TaskEventKind::Created,
                    from_status: None,
                    to_status: Some(TaskStatus::Todo),
                    from_assignee_id: None,
                    to_assignee_id: None,
                    task_version: 1,
                    created_at_ms: now,
                })
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TaskCreated,
                &target_id,
                message_id,
                &[],
                now,
            )
            .await?;
            let anchor_text = MessageStore::new(connection).body_text(message_id).await?;

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
        })
        .await
    }

    /// Claim an unowned Task. A second actor receives a typed concurrency
    /// conflict; the current assignee may repeat the claim idempotently.
    pub async fn claim_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = TaskStore::new(connection);
            let current =
                store
                    .find_by_message(message_id)
                    .await?
                    .ok_or_else(|| CollabError::NotFound {
                        entity: "task",
                        id: message_id.to_owned(),
                    })?;
            require_target_access(connection, &current.target_id, actor_id).await?;
            if current.status == TaskStatus::Done {
                return Err(CollabError::TaskTransitionDenied {
                    message_id: message_id.to_owned(),
                    status: current.status.as_str().to_owned(),
                });
            }
            if let Some(assignee_id) = &current.assignee_id {
                if assignee_id == actor_id {
                    return Ok(current);
                }
                return Err(CollabError::TaskAlreadyClaimed {
                    message_id: message_id.to_owned(),
                    assignee_id: assignee_id.clone(),
                });
            }

            let next_version = current.version + 1;
            store
                .claim(message_id, actor_id, next_version, current.version, now)
                .await?;
            store
                .record_event(&TaskEvent {
                    message_id,
                    actor_id,
                    kind: TaskEventKind::Claimed,
                    from_status: Some(current.status),
                    to_status: Some(TaskStatus::InProgress),
                    from_assignee_id: None,
                    to_assignee_id: Some(actor_id),
                    task_version: next_version,
                    created_at_ms: now,
                })
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TaskUpdated,
                &current.target_id,
                message_id,
                &[],
                now,
            )
            .await?;

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
        })
        .await
    }

    /// Release a Task currently claimed by this actor while preserving its
    /// independent status. The expected version fences stale UI writes.
    pub async fn unclaim_task(
        &self,
        message_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Task> {
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = TaskStore::new(connection);
            let current =
                store
                    .find_by_message(message_id)
                    .await?
                    .ok_or_else(|| CollabError::NotFound {
                        entity: "task",
                        id: message_id.to_owned(),
                    })?;
            require_target_access(connection, &current.target_id, actor_id).await?;
            if current.version != expected_version {
                return Err(CollabError::TaskVersionConflict {
                    message_id: message_id.to_owned(),
                    expected: expected_version,
                    actual: current.version,
                });
            }
            let Some(assignee_id) = current.assignee_id.as_deref() else {
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
            store
                .unclaim(message_id, actor_id, next_version, current.version, now)
                .await?;
            store
                .record_event(&TaskEvent {
                    message_id,
                    actor_id,
                    kind: TaskEventKind::Unclaimed,
                    from_status: Some(current.status),
                    to_status: Some(current.status),
                    from_assignee_id: Some(actor_id),
                    to_assignee_id: None,
                    task_version: next_version,
                    created_at_ms: now,
                })
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TaskUpdated,
                &current.target_id,
                message_id,
                &[],
                now,
            )
            .await?;

            Ok(Task {
                assignee_id: None,
                version: next_version,
                updated_at_ms: now,
                ..current
            })
        })
        .await
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
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = TaskStore::new(connection);
            let current =
                store
                    .find_by_message(message_id)
                    .await?
                    .ok_or_else(|| CollabError::NotFound {
                        entity: "task",
                        id: message_id.to_owned(),
                    })?;
            require_target_access(connection, &current.target_id, actor_id).await?;
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
            {
                let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
                if !Membership::require(connection, &current.target_id, &actor)
                    .await?
                    .is_owner()
                {
                    return Err(CollabError::PermissionDenied {
                        actor_id: actor_id.to_owned(),
                        action: "update another actor's task in",
                        target_id: current.target_id,
                    });
                }
            }
            if current.status == status {
                return Ok(current);
            }
            if !task_transition_allowed(current.status, status) {
                return Err(CollabError::TaskTransitionDenied {
                    message_id: message_id.to_owned(),
                    status: current.status.as_str().to_owned(),
                });
            }

            let next_version = current.version + 1;
            store
                .apply_status(message_id, status, next_version, current.version, now)
                .await?;
            store
                .record_event(&TaskEvent {
                    message_id,
                    actor_id,
                    kind: TaskEventKind::StatusChanged,
                    from_status: Some(current.status),
                    to_status: Some(status),
                    from_assignee_id: current.assignee_id.as_deref(),
                    to_assignee_id: current.assignee_id.as_deref(),
                    task_version: next_version,
                    created_at_ms: now,
                })
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TaskUpdated,
                &current.target_id,
                message_id,
                &[],
                now,
            )
            .await?;

            Ok(Task {
                status,
                version: next_version,
                updated_at_ms: now,
                ..current
            })
        })
        .await
    }
}

impl CollabCore {
    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if let Some(target_id) = target_id {
                require_target_access(connection, target_id, actor_id).await?;
            }
            TaskStore::new(connection)
                .tasks_for_actor(actor_id, target_id)
                .await
        })
        .await
    }
}
