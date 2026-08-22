use serde::{Deserialize, Serialize};

use crate::{CollabError, Result};

/// A committed immutable text message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    /// Global monotonic database sequence. Expose it as a decimal string over
    /// NAPI so JavaScript never loses integer precision.
    pub seq: i64,
    pub id: String,
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
    pub created_at_ms: i64,
}

/// Authoritative tail page of one target: the exact total message count plus
/// the latest messages in ascending order, read from one consistent snapshot.
/// `count` is always exact, even when it exceeds `messages.len()`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MessageTail {
    pub count: i64,
    pub messages: Vec<Message>,
}

/// One idempotent send command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendMessageRequest {
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
}

/// Durable send result plus post-commit runtime effects.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendMessageResult {
    pub message: Message,
    pub recipient_ids: Vec<String>,
    pub wake_agent_ids: Vec<String>,
    pub replayed: bool,
}

/// The stored body envelope of one immutable Message.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct StoredTextBody {
    pub(crate) kind: String,
    pub(crate) text: String,
}

/// Decode the plain text of one stored body, naming `context` on failure.
pub(crate) fn stored_text(body_json: &str, context: &str) -> Result<String> {
    serde_json::from_str::<StoredTextBody>(body_json)
        .map(|body| body.text)
        .map_err(|error| CollabError::Database(format!("{context} is malformed: {error}")))
}

/// One immutable Message staged for insertion: the stored body is encoded and
/// the timestamp fixed before the write transaction begins.
pub(crate) struct NewMessage {
    pub(crate) id: String,
    pub(crate) target_id: String,
    pub(crate) author_id: String,
    pub(crate) client_request_id: String,
    pub(crate) body_json: String,
    pub(crate) created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendFailpoint {
    None,
    #[cfg(test)]
    AfterMessageInsert,
}
