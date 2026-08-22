use turso::{Connection, Row};

use crate::db::{FromRow, QueryRows, assert_one_row};
use crate::message::StoredTextBody;
use crate::{CollabError, Result, Task, TaskStatus};

use super::model::{NewTask, TaskEvent, TaskTransition};

/// Column order of the canonical Task projection: the Task row plus the anchor
/// Message body through a LEFT JOIN. Shared by every SELECT in this store.
const TASK_COLUMNS: &str =
    "task.message_id, task.target_id, task.number, task.status, task.assignee_id,
     task.version, task.created_at_ms, task.updated_at_ms, message.body_json";

struct TaskRow {
    message_id: String,
    target_id: String,
    number: i64,
    status: String,
    assignee_id: Option<String>,
    version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    body_json: Option<String>,
}

impl FromRow for TaskRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            message_id: row.get(0)?,
            target_id: row.get(1)?,
            number: row.get(2)?,
            status: row.get(3)?,
            assignee_id: row.get(4)?,
            version: row.get(5)?,
            created_at_ms: row.get(6)?,
            updated_at_ms: row.get(7)?,
            body_json: row.get(8)?,
        })
    }
}

impl TaskRow {
    fn into_task(self) -> Result<Task> {
        let Self {
            message_id,
            target_id,
            number,
            status,
            assignee_id,
            version,
            created_at_ms,
            updated_at_ms,
            body_json,
        } = self;
        let status = TaskStatus::parse(&status).ok_or_else(|| {
            CollabError::Database(format!("task '{message_id}' has invalid status '{status}'"))
        })?;
        let anchor_text = body_json
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
            target_id,
            number,
            status,
            assignee_id,
            version,
            created_at_ms,
            updated_at_ms,
            anchor_text,
        })
    }
}

struct NumberRow(i64);

impl FromRow for NumberRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

pub(crate) struct TaskStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> TaskStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn find_by_message(&self, message_id: &str) -> Result<Option<Task>> {
        self.connection
            .query_row::<TaskRow>(
                &format!(
                    "SELECT {TASK_COLUMNS}
                     FROM tasks task
                     LEFT JOIN messages message ON message.id = task.message_id
                     WHERE task.message_id = ?1"
                ),
                [message_id],
            )
            .await?
            .map(TaskRow::into_task)
            .transpose()
    }

    /// List Task rows visible to `actor_id`, optionally narrowed to one exact
    /// target. Visibility follows the permission target, so Thread tasks ride
    /// their parent membership.
    pub(crate) async fn tasks_for_actor(
        &self,
        actor_id: &str,
        target_id: Option<&str>,
    ) -> Result<Vec<Task>> {
        let rows: Vec<TaskRow> = if let Some(target_id) = target_id {
            self.connection
                .query_rows(
                    &format!(
                        "SELECT {TASK_COLUMNS}
                         FROM tasks task
                         JOIN v_target_access access
                           ON access.target_id = task.target_id AND access.actor_id = ?1
                         LEFT JOIN messages message ON message.id = task.message_id
                         WHERE task.target_id = ?2
                         ORDER BY task.number"
                    ),
                    (actor_id, target_id),
                )
                .await?
        } else {
            self.connection
                .query_rows(
                    &format!(
                        "SELECT {TASK_COLUMNS}
                         FROM tasks task
                         JOIN v_target_access access
                           ON access.target_id = task.target_id AND access.actor_id = ?1
                         LEFT JOIN messages message ON message.id = task.message_id
                         ORDER BY task.target_id, task.number"
                    ),
                    [actor_id],
                )
                .await?
        };
        rows.into_iter().map(TaskRow::into_task).collect()
    }

    /// Allocate the next target-local Task number inside the write
    /// transaction.
    pub(crate) async fn next_number(&self, target_id: &str) -> Result<i64> {
        self.connection
            .execute(
                "INSERT INTO target_counters (target_id, next_task_number)
                 VALUES (?1, 1)
                 ON CONFLICT(target_id) DO NOTHING",
                [target_id],
            )
            .await?;
        let number = self
            .connection
            .query_row::<NumberRow>(
                "SELECT next_task_number FROM target_counters WHERE target_id = ?1",
                [target_id],
            )
            .await?
            .map(|row| row.0)
            .ok_or_else(|| {
                CollabError::Database("target task counter disappeared inside transaction".into())
            })?;
        self.connection
            .execute(
                "UPDATE target_counters SET next_task_number = ?2 WHERE target_id = ?1",
                (target_id, number + 1),
            )
            .await?;
        Ok(number)
    }

    pub(crate) async fn insert(&self, task: &NewTask<'_>) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO tasks
                 (message_id, target_id, number, status, assignee_id, version, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, 'todo', NULL, 1, ?4, ?4)",
                (
                    task.message_id,
                    task.target_id,
                    task.number,
                    task.created_at_ms,
                ),
            )
            .await?;
        Ok(())
    }

    /// Compare-and-set one fresh claim onto a currently unowned Task.
    pub(crate) async fn claim(
        &self,
        message_id: &str,
        actor_id: &str,
        next_version: i64,
        expected_version: i64,
        now: i64,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE tasks
                 SET assignee_id = ?2, status = 'in_progress', version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND assignee_id IS NULL AND version = ?5",
                (message_id, actor_id, next_version, now, expected_version),
            )
            .await?;
        assert_one_row(changed, "task claim compare-and-set")
    }

    /// Compare-and-set the release of a Task claimed by this actor.
    pub(crate) async fn unclaim(
        &self,
        message_id: &str,
        actor_id: &str,
        next_version: i64,
        expected_version: i64,
        now: i64,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE tasks
                 SET assignee_id = NULL, version = ?2, updated_at_ms = ?3
                 WHERE message_id = ?1 AND assignee_id = ?4 AND version = ?5",
                (message_id, next_version, now, actor_id, expected_version),
            )
            .await?;
        assert_one_row(changed, "task unclaim compare-and-set")
    }

    /// Compare-and-set one lifecycle status at the expected version.
    pub(crate) async fn apply_status(
        &self,
        message_id: &str,
        status: TaskStatus,
        next_version: i64,
        expected_version: i64,
        now: i64,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE tasks
                 SET status = ?2, version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND version = ?5",
                (
                    message_id,
                    status.as_str(),
                    next_version,
                    now,
                    expected_version,
                ),
            )
            .await?;
        assert_one_row(changed, "task status compare-and-set")
    }

    pub(crate) async fn record_event(&self, event: &TaskEvent<'_>) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO task_events
                 (message_id, actor_id, event_type, from_status, to_status,
                  from_assignee_id, to_assignee_id, task_version, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    event.message_id,
                    event.actor_id,
                    event.kind.as_str(),
                    event.from_status.map(TaskStatus::as_str),
                    event.to_status.map(TaskStatus::as_str),
                    event.from_assignee_id,
                    event.to_assignee_id,
                    event.task_version,
                    event.created_at_ms,
                ),
            )
            .await?;
        Ok(())
    }
}

impl TaskStore<'_> {
    /// Persist one committed TaskTransition: the CAS row update and its audit
    /// event in one call. The change notification stays with the caller.
    pub(crate) async fn apply(&self, transition: &TaskTransition, now: i64) -> Result<()> {
        let next_assignee = transition
            .next_assignee_id
            .as_ref()
            .map(|assignee| assignee.as_deref());
        match (transition.next_status, next_assignee) {
            (Some(TaskStatus::InProgress), Some(Some(assignee))) => {
                self.claim(
                    &transition.message_id,
                    assignee,
                    transition.next_version,
                    transition.next_version - 1,
                    now,
                )
                .await?;
            }
            (None, Some(None)) => {
                self.unclaim(
                    &transition.message_id,
                    &transition.actor_id,
                    transition.next_version,
                    transition.next_version - 1,
                    now,
                )
                .await?;
            }
            (Some(status), None) => {
                self.apply_status(
                    &transition.message_id,
                    status,
                    transition.next_version,
                    transition.next_version - 1,
                    now,
                )
                .await?;
            }
            _ => {
                return Err(crate::CollabError::Database(format!(
                    "task transition for '{}' carries an unsupported row delta",
                    transition.message_id
                )));
            }
        }
        self.record_event(&transition.event(now)).await?;
        Ok(())
    }
}
