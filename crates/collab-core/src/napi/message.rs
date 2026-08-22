//! NAPI bridge for the message vertical.

use crate::{Message, SendMessageRequest, SendMessageResult};
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct SendMessageInput {
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
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

#[napi(object)]
pub struct JsMessageTail {
    /// Exact total message count of the target, as a decimal string so
    /// JavaScript never loses integer precision.
    pub count: String,
    pub messages: Vec<JsMessage>,
}

#[napi(object)]
pub struct JsSendMessageResult {
    pub message: JsMessage,
    pub recipient_ids: Vec<String>,
    pub wake_agent_ids: Vec<String>,
    pub replayed: bool,
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

#[napi]
impl CollabHandle {
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
    pub async fn read_message(
        &self,
        actor_id: String,
        target_id: String,
        message_id: String,
    ) -> Result<JsMessage> {
        self.core
            .read_message(&actor_id, &target_id, &message_id)
            .await
            .map(JsMessage::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn read_messages(
        &self,
        actor_id: String,
        target_id: String,
        after_seq: String,
        limit: u32,
    ) -> Result<Vec<JsMessage>> {
        let after_seq = parse_i64("after_seq", &after_seq)?;
        self.core
            .read_messages(&actor_id, &target_id, after_seq, limit)
            .await
            .map(|messages| messages.into_iter().map(JsMessage::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn read_messages_tail(
        &self,
        actor_id: String,
        target_id: String,
        limit: u32,
    ) -> Result<JsMessageTail> {
        self.core
            .read_messages_tail(&actor_id, &target_id, limit)
            .await
            .map(|tail| JsMessageTail {
                count: tail.count.to_string(),
                messages: tail.messages.into_iter().map(JsMessage::from).collect(),
            })
            .map_err(to_napi_error)
    }
}
