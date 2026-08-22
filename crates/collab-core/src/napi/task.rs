//! NAPI bridge for the task vertical.

use crate::{Task, TaskStatus};
use napi::{Error, Result, Status};
use napi_derive::napi;

use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct JsTask {
    pub message_id: String,
    pub target_id: String,
    pub number: String,
    pub status: String,
    pub assignee_id: Option<String>,
    pub version: String,
    pub created_at_ms: f64,
    pub updated_at_ms: f64,
    pub anchor_text: Option<String>,
}

impl From<Task> for JsTask {
    fn from(task: Task) -> Self {
        let status = match task.status {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::InReview => "in_review",
            TaskStatus::Done => "done",
        };
        Self {
            message_id: task.message_id,
            target_id: task.target_id,
            number: task.number.to_string(),
            status: status.into(),
            assignee_id: task.assignee_id,
            version: task.version.to_string(),
            created_at_ms: task.created_at_ms as f64,
            updated_at_ms: task.updated_at_ms as f64,
            anchor_text: task.anchor_text,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_task(&self, message_id: String, actor_id: String) -> Result<JsTask> {
        self.core
            .create_task(&message_id, &actor_id)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn claim_task(&self, message_id: String, actor_id: String) -> Result<JsTask> {
        self.core
            .claim_task(&message_id, &actor_id)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_tasks(
        &self,
        actor_id: String,
        target_id: Option<String>,
    ) -> Result<Vec<JsTask>> {
        self.core
            .list_tasks(&actor_id, target_id.as_deref())
            .await
            .map(|tasks| tasks.into_iter().map(JsTask::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn unclaim_task(
        &self,
        message_id: String,
        actor_id: String,
        expected_version: String,
    ) -> Result<JsTask> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        self.core
            .unclaim_task(&message_id, &actor_id, expected_version)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_task_status(
        &self,
        message_id: String,
        actor_id: String,
        status: String,
        expected_version: String,
    ) -> Result<JsTask> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        let status = TaskStatus::parse(&status).ok_or_else(|| {
            Error::new(
                Status::InvalidArg,
                "[invalid_argument] status must be todo, in_progress, in_review, or done",
            )
        })?;
        self.core
            .update_task_status(&message_id, &actor_id, status, expected_version)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }
}
