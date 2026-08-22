use serde::{Deserialize, Serialize};

use crate::{CollabError, Result};

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
