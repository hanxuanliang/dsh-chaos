//! Task projection, claim ownership, and lifecycle transitions.

mod model;
pub(crate) mod store;

pub use model::{Task, TaskStatus};

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::message::store::MessageStore;
use crate::target::AccessGrant;
use crate::{Actor, ChangeKind, CollabCore, CollabError, Result, TargetKind, now_ms};

use model::{LoadedTask, NewTask, TaskDecision, TaskEvent, TaskEventKind};
use store::TaskStore;

impl CollabCore {
    /// Convert a committed top-level Message to a Task with a target-local
    /// monotonic task number.
    pub async fn create_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        let now = now_ms()?;
        self.write(async |connection| {
            let target_id = MessageStore::new(connection).target_of(message_id).await?;
            let grant = AccessGrant::require(connection, &target_id, actor_id).await?;
            if grant.route.kind == TargetKind::Thread {
                return Err(CollabError::InvalidArgument(
                    "Thread replies cannot become Tasks".into(),
                ));
            }

            let store = TaskStore::new(connection);
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
            ChangeStore::new(connection)
                .insert_target_change(ChangeKind::TaskCreated, &target_id, message_id, &[], now)
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
            let loaded = load_task(connection, message_id, actor_id).await?;
            let task = loaded.task.clone();
            let task = match loaded.claim(now)? {
                TaskDecision::Idempotent => task,
                TaskDecision::Transition(transition) => {
                    apply_task_transition(connection, &transition, now).await?;
                    transition.task_after
                }
            };
            Ok(task)
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
            let loaded = load_task(connection, message_id, actor_id).await?;
            let task = loaded.task.clone();
            let task = match loaded.unclaim(expected_version, now)? {
                TaskDecision::Idempotent => task,
                TaskDecision::Transition(transition) => {
                    apply_task_transition(connection, &transition, now).await?;
                    transition.task_after
                }
            };
            Ok(task)
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
            let loaded = load_task(connection, message_id, actor_id).await?;
            let task = loaded.task.clone();
            let task = match loaded.change_status(status, expected_version, now)? {
                TaskDecision::Idempotent => task,
                TaskDecision::Transition(transition) => {
                    apply_task_transition(connection, &transition, now).await?;
                    transition.task_after
                }
            };
            Ok(task)
        })
        .await
    }

    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if let Some(target_id) = target_id {
                AccessGrant::require(connection, target_id, actor_id).await?;
            }
            TaskStore::new(connection)
                .tasks_for_actor(actor_id, target_id)
                .await
        })
        .await
    }
}

/// Assemble one Task's evidence: the loaded row plus the acting actor's
/// proven access.
async fn load_task(
    connection: &turso::Connection,
    message_id: &str,
    actor_id: &str,
) -> Result<LoadedTask> {
    let task = TaskStore::new(connection)
        .find_by_message(message_id)
        .await?
        .ok_or_else(|| LoadedTask::not_found(message_id))?;
    let grant = AccessGrant::require(connection, &task.target_id, actor_id).await?;
    Ok(LoadedTask { task, grant })
}

/// Persist one committed decision: the fenced row update, its audit event,
/// and the change notification, all in the caller's transaction.
async fn apply_task_transition(
    connection: &turso::Connection,
    transition: &model::TaskTransition,
    now: i64,
) -> Result<()> {
    let store = TaskStore::new(connection);
    store.apply(transition, now).await?;
    if transition.publish {
        ChangeStore::new(connection)
            .insert_target_change(
                ChangeKind::TaskUpdated,
                &transition.target_id,
                &transition.message_id,
                &[],
                now,
            )
            .await?;
    }
    Ok(())
}
