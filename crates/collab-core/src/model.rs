use serde::{Deserialize, Serialize};

use crate::{IdentityContext, Message, RuntimeBinding};

/// One current runtime whose durable inbox still needs a level-triggered
/// notification. The watermark is the newest authorized, not-yet-model-seen
/// delivery at the time of the scan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingWake {
    pub binding: RuntimeBinding,
    pub pending_seq: i64,
}

/// One message returned by an inbox check.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxMessage {
    pub delivery_id: String,
    pub message: Message,
}

/// A durable record of the exact results returned by one check call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxBatch {
    pub id: Option<String>,
    pub agent_id: String,
    pub session_id: String,
    pub generation: i64,
    pub messages: Vec<InboxMessage>,
    pub contexts: Vec<IdentityContext>,
    pub checked_at_ms: i64,
}
