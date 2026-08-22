use turso::{Connection, Row};

use crate::db::{FromRow, QueryRows, require_scalar_row};
use crate::{ActorKind, CollabError, Message, Result};

use super::model::{NewMessage, StoredTextBody, stored_text};

/// Column order of the canonical Message projection, shared by every SELECT
/// in this store.
const MESSAGE_COLUMNS: &str =
    "seq, id, target_id, author_id, client_request_id, body_json, created_at_ms";

struct MessageRow {
    seq: i64,
    id: String,
    target_id: String,
    author_id: String,
    client_request_id: String,
    body_json: String,
    created_at_ms: i64,
}

impl FromRow for MessageRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            seq: row.get(0)?,
            id: row.get(1)?,
            target_id: row.get(2)?,
            author_id: row.get(3)?,
            client_request_id: row.get(4)?,
            body_json: row.get(5)?,
            created_at_ms: row.get(6)?,
        })
    }
}

impl MessageRow {
    fn into_message(self) -> Result<Message> {
        Ok(Message {
            seq: self.seq,
            id: self.id,
            target_id: self.target_id,
            author_id: self.author_id,
            client_request_id: self.client_request_id,
            text: stored_text(&self.body_json, "message body")?,
            created_at_ms: self.created_at_ms,
        })
    }
}

/// One active recipient selected for a Message: id plus Actor kind for the
/// wake-watermark split.
pub(crate) struct Recipient {
    id: String,
    kind: String,
}

impl FromRow for Recipient {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
        })
    }
}

struct CountRow(i64);

impl FromRow for CountRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct SeqRow(i64);

impl FromRow for SeqRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct TargetOfRow(String);

impl FromRow for TargetOfRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct BodyRow(String);

impl FromRow for BodyRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct TargetAuthorRow {
    target_id: String,
    author_id: String,
}

impl FromRow for TargetAuthorRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            target_id: row.get(0)?,
            author_id: row.get(1)?,
        })
    }
}

pub(crate) struct MessageStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> MessageStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Load the Message with `message_id` inside the exact `target_id`,
    /// failing when absent.
    pub(crate) async fn require_in_target(
        &self,
        target_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        self.connection
            .query_row::<MessageRow>(
                &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1 AND target_id = ?2"),
                (message_id, target_id),
            )
            .await?
            .map(MessageRow::into_message)
            .transpose()?
            .ok_or_else(|| CollabError::NotFound {
                entity: "message in exact target",
                id: message_id.to_owned(),
            })
    }

    /// Read an ascending page from one exact target after a global sequence.
    pub(crate) async fn page_after(
        &self,
        target_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        self.connection
            .query_rows::<MessageRow>(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages
                     WHERE target_id = ?1 AND seq > ?2
                     ORDER BY seq
                     LIMIT ?3"
                ),
                (target_id, after_seq, i64::from(limit)),
            )
            .await?
            .into_iter()
            .map(MessageRow::into_message)
            .collect()
    }

    pub(crate) async fn count_in_target(&self, target_id: &str) -> Result<i64> {
        let row = self
            .connection
            .query_row::<CountRow>(
                "SELECT COUNT(*) FROM messages WHERE target_id = ?1",
                [target_id],
            )
            .await?;
        Ok(require_scalar_row(row, "message count")?.0)
    }

    /// Read the true latest `limit` messages, ascending.
    pub(crate) async fn tail(&self, target_id: &str, limit: u32) -> Result<Vec<Message>> {
        self.connection
            .query_rows::<MessageRow>(
                &format!(
                    "SELECT {MESSAGE_COLUMNS}
                     FROM (
                         SELECT {MESSAGE_COLUMNS}
                         FROM messages
                         WHERE target_id = ?1
                         ORDER BY seq DESC
                         LIMIT ?2
                     )
                     ORDER BY seq"
                ),
                (target_id, i64::from(limit)),
            )
            .await?
            .into_iter()
            .map(MessageRow::into_message)
            .collect()
    }

    /// Idempotency lookup for one author's client request id.
    pub(crate) async fn find_by_request(
        &self,
        author_id: &str,
        client_request_id: &str,
    ) -> Result<Option<Message>> {
        self.connection
            .query_row::<MessageRow>(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages
                     WHERE author_id = ?1 AND client_request_id = ?2"
                ),
                (author_id, client_request_id),
            )
            .await?
            .map(MessageRow::into_message)
            .transpose()
    }

    /// The recipient snapshot recorded for `message_id`, split into every
    /// recipient and the Agent subset that carries a wake watermark.
    pub(crate) async fn recorded_recipients(
        &self,
        message_id: &str,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let rows: Vec<Recipient> = self
            .connection
            .query_rows(
                "SELECT d.recipient_id, a.kind
                 FROM deliveries d JOIN actors a ON a.id = d.recipient_id
                 WHERE d.message_id = ?1
                 ORDER BY d.recipient_id",
                [message_id],
            )
            .await?;
        let mut recipient_ids = Vec::with_capacity(rows.len());
        let mut wake_agent_ids = Vec::new();
        for recipient in rows {
            if recipient.kind == ActorKind::Agent.as_str() {
                wake_agent_ids.push(recipient.id.clone());
            }
            recipient_ids.push(recipient.id);
        }
        Ok((recipient_ids, wake_agent_ids))
    }

    pub(crate) async fn target_of(&self, message_id: &str) -> Result<String> {
        self.connection
            .query_row::<TargetOfRow>("SELECT target_id FROM messages WHERE id = ?1", [message_id])
            .await?
            .map(|row| row.0)
            .ok_or_else(|| CollabError::NotFound {
                entity: "message",
                id: message_id.to_owned(),
            })
    }

    pub(crate) async fn body_text(&self, message_id: &str) -> Result<Option<String>> {
        self.connection
            .query_row::<BodyRow>("SELECT body_json FROM messages WHERE id = ?1", [message_id])
            .await?
            .map(|row| {
                serde_json::from_str::<StoredTextBody>(&row.0)
                    .map_err(|error| {
                        CollabError::Database(format!(
                            "message '{message_id}' has invalid body: {error}"
                        ))
                    })
                    .map(|body| body.text)
            })
            .transpose()
    }

    pub(crate) async fn target_and_author(&self, message_id: &str) -> Result<(String, String)> {
        self.connection
            .query_row::<TargetAuthorRow>(
                "SELECT target_id, author_id FROM messages WHERE id = ?1",
                [message_id],
            )
            .await?
            .map(|row| (row.target_id, row.author_id))
            .ok_or_else(|| CollabError::NotFound {
                entity: "message",
                id: message_id.to_owned(),
            })
    }

    /// Insert `message`, returning its global sequence.
    pub(crate) async fn insert(&self, message: &NewMessage) -> Result<i64> {
        self.connection
            .query_row::<SeqRow>(
                "INSERT INTO messages
                 (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 RETURNING seq",
                (
                    message.id.as_str(),
                    message.target_id.as_str(),
                    message.author_id.as_str(),
                    message.client_request_id.as_str(),
                    message.body_json.as_str(),
                    message.created_at_ms,
                ),
            )
            .await?
            .map(|row| row.0)
            .ok_or_else(|| CollabError::Database("message insert returned no sequence".into()))
    }

    /// Active members of one Channel/Direct, excluding the author.
    pub(crate) async fn active_target_recipients(
        &self,
        target_id: &str,
        author_id: &str,
    ) -> Result<Vec<Recipient>> {
        self.connection
            .query_rows(
                "SELECT a.id, a.kind
                 FROM memberships m
                 JOIN actors a ON a.id = m.actor_id
                 WHERE m.target_id = ?1
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                (target_id, author_id),
            )
            .await
    }

    /// Following, still-member recipients of one Thread, excluding the author.
    pub(crate) async fn active_thread_recipients(
        &self,
        thread_target_id: &str,
        author_id: &str,
        permission_target_id: &str,
    ) -> Result<Vec<Recipient>> {
        self.connection
            .query_rows(
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
                (thread_target_id, author_id, permission_target_id),
            )
            .await
    }

    /// Commit the recipient snapshot: one Delivery per recipient and a
    /// level-triggered wake watermark per Agent, returning both id lists.
    pub(crate) async fn record_deliveries(
        &self,
        message: &NewMessage,
        message_seq: i64,
        recipients: &[Recipient],
    ) -> Result<(Vec<String>, Vec<String>)> {
        let mut recipient_ids = Vec::with_capacity(recipients.len());
        let mut wake_agent_ids = Vec::new();
        for recipient in recipients {
            self.connection
                .execute(
                    "INSERT INTO deliveries
                     (id, message_id, message_seq, target_id, recipient_id, committed_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        crate::new_id(),
                        message.id.as_str(),
                        message_seq,
                        message.target_id.as_str(),
                        recipient.id.as_str(),
                        message.created_at_ms,
                    ),
                )
                .await?;
            if recipient.kind == ActorKind::Agent.as_str() {
                self.connection
                    .execute(
                        "INSERT INTO agent_wake_state
                         (agent_id, pending_seq, notified_seq, notified_generation, attempt_count)
                         VALUES (?1, ?2, 0, 0, 0)
                         ON CONFLICT(agent_id) DO UPDATE SET
                           pending_seq = MAX(agent_wake_state.pending_seq, excluded.pending_seq)",
                        (recipient.id.as_str(), message_seq),
                    )
                    .await?;
                wake_agent_ids.push(recipient.id.clone());
            }
            recipient_ids.push(recipient.id.clone());
        }
        Ok((recipient_ids, wake_agent_ids))
    }
}
