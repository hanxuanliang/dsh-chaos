//! Coarse-grained NAPI bridge for the Rust collab core.

use std::sync::Arc;

use crate::{
    Actor, CollabCore, CollabError, InboxBatch, Message, RuntimeBinding, SendMessageRequest,
    SendMessageResult, Target, Task, TaskStatus,
};
use napi::{Error, Result, Status};
use napi_derive::napi;

#[napi]
pub struct CollabHandle {
    core: Arc<CollabCore>,
}

#[napi(object)]
pub struct SendMessageInput {
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
}

#[napi(object)]
pub struct JsActor {
    pub id: String,
    pub kind: String,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsTarget {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub created_by: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsMessage {
    pub seq: String,
    pub id: String,
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsSendMessageResult {
    pub message: JsMessage,
    pub recipient_ids: Vec<String>,
    pub wake_agent_ids: Vec<String>,
    pub replayed: bool,
}

#[napi(object)]
pub struct JsRuntimeBinding {
    pub agent_id: String,
    pub session_id: String,
    pub generation: String,
    pub provider: String,
    pub model: String,
    pub preset: String,
    pub bound_at_ms: f64,
}

#[napi(object)]
pub struct JsInboxMessage {
    pub delivery_id: String,
    pub message: JsMessage,
}

#[napi(object)]
pub struct JsInboxBatch {
    pub id: Option<String>,
    pub agent_id: String,
    pub session_id: String,
    pub generation: String,
    pub messages: Vec<JsInboxMessage>,
    pub checked_at_ms: f64,
}

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
}

#[napi]
pub async fn open_collab(path: String) -> Result<CollabHandle> {
    let core = CollabCore::open(path).await.map_err(to_napi_error)?;
    Ok(CollabHandle {
        core: Arc::new(core),
    })
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn close(&self) -> Result<()> {
        self.core.close().await.map_err(to_napi_error)
    }

    #[napi]
    pub async fn create_user(&self, handle: String, display_name: String) -> Result<JsActor> {
        self.core
            .create_user(&handle, &display_name)
            .await
            .map(JsActor::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn create_agent(
        &self,
        handle: String,
        display_name: String,
        workspace_path: String,
    ) -> Result<JsActor> {
        self.core
            .create_agent(&handle, &display_name, &workspace_path)
            .await
            .map(JsActor::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn create_channel(&self, name: String, creator_id: String) -> Result<JsTarget> {
        self.core
            .create_channel(&name, &creator_id)
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn add_member(
        &self,
        target_id: String,
        actor_id: String,
        added_by: String,
    ) -> Result<()> {
        self.core
            .add_member(&target_id, &actor_id, &added_by)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn send_message(&self, input: SendMessageInput) -> Result<JsSendMessageResult> {
        self.core
            .send_message(SendMessageRequest {
                target_id: input.target_id,
                author_id: input.author_id,
                client_request_id: input.client_request_id,
                text: input.text,
            })
            .await
            .map(JsSendMessageResult::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn bind_runtime(
        &self,
        agent_id: String,
        session_id: String,
        provider: String,
        model: String,
        preset: String,
    ) -> Result<JsRuntimeBinding> {
        self.core
            .bind_runtime(&agent_id, &session_id, &provider, &model, &preset)
            .await
            .map(JsRuntimeBinding::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn check_inbox(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
        limit: u32,
    ) -> Result<JsInboxBatch> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .check_inbox(&agent_id, generation, &session_id, limit)
            .await
            .map(JsInboxBatch::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn mark_model_seen(
        &self,
        batch_id: String,
        agent_id: String,
        generation: String,
        session_id: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .mark_model_seen(&batch_id, &agent_id, generation, &session_id)
            .await
            .map_err(to_napi_error)
    }

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
}

impl From<Actor> for JsActor {
    fn from(actor: Actor) -> Self {
        Self {
            id: actor.id,
            kind: format!("{:?}", actor.kind).to_ascii_lowercase(),
            handle: actor.handle,
            display_name: actor.display_name,
            created_at_ms: actor.created_at_ms as f64,
        }
    }
}

impl From<Target> for JsTarget {
    fn from(target: Target) -> Self {
        Self {
            id: target.id,
            kind: format!("{:?}", target.kind).to_ascii_lowercase(),
            name: target.name,
            created_by: target.created_by,
            created_at_ms: target.created_at_ms as f64,
        }
    }
}

impl From<Message> for JsMessage {
    fn from(message: Message) -> Self {
        Self {
            seq: message.seq.to_string(),
            id: message.id,
            target_id: message.target_id,
            author_id: message.author_id,
            client_request_id: message.client_request_id,
            text: message.text,
            created_at_ms: message.created_at_ms as f64,
        }
    }
}

impl From<SendMessageResult> for JsSendMessageResult {
    fn from(result: SendMessageResult) -> Self {
        Self {
            message: result.message.into(),
            recipient_ids: result.recipient_ids,
            wake_agent_ids: result.wake_agent_ids,
            replayed: result.replayed,
        }
    }
}

impl From<RuntimeBinding> for JsRuntimeBinding {
    fn from(binding: RuntimeBinding) -> Self {
        Self {
            agent_id: binding.agent_id,
            session_id: binding.session_id,
            generation: binding.generation.to_string(),
            provider: binding.provider,
            model: binding.model,
            preset: binding.preset,
            bound_at_ms: binding.bound_at_ms as f64,
        }
    }
}

impl From<InboxBatch> for JsInboxBatch {
    fn from(batch: InboxBatch) -> Self {
        Self {
            id: batch.id,
            agent_id: batch.agent_id,
            session_id: batch.session_id,
            generation: batch.generation.to_string(),
            messages: batch
                .messages
                .into_iter()
                .map(|item| JsInboxMessage {
                    delivery_id: item.delivery_id,
                    message: item.message.into(),
                })
                .collect(),
            checked_at_ms: batch.checked_at_ms as f64,
        }
    }
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
        }
    }
}

fn parse_i64(name: &str, value: &str) -> Result<i64> {
    value.parse().map_err(|_| {
        Error::new(
            Status::InvalidArg,
            format!("[invalid_argument] {name} must be a signed 64-bit decimal string"),
        )
    })
}

fn to_napi_error(error: CollabError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("[{}] {error}", error.code()),
    )
}
