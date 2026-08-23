//! Wake ledger, inbox batch, and model-seen persistence.

use turso::{Connection, Row};

use crate::actor::ActorKind;
use crate::db::{FromRow, QueryRows};
use crate::message::NewMessage;
use crate::message::Recipient;
use crate::message::stored_text;
use crate::{CollabError, InboxMessage, Message, PendingWake, Result};

/// The authorized, not-yet-model-seen delivery ledger for one Agent.
pub(crate) struct DeliveryStore<'connection> {
    connection: &'connection Connection,
}

/// Column projection for one queued inbox delivery joined with its Message.
struct InboxMessageRow {
    delivery_id: String,
    seq: i64,
    id: String,
    target_id: String,
    author_id: String,
    client_request_id: String,
    body_json: String,
    created_at_ms: i64,
}

impl FromRow for InboxMessageRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            delivery_id: row.get(0)?,
            seq: row.get(1)?,
            id: row.get(2)?,
            target_id: row.get(3)?,
            author_id: row.get(4)?,
            client_request_id: row.get(5)?,
            body_json: row.get(6)?,
            created_at_ms: row.get(7)?,
        })
    }
}

impl InboxMessageRow {
    fn into_message(self) -> Result<InboxMessage> {
        let text = stored_text(&self.body_json, "message body")?;
        Ok(InboxMessage {
            delivery_id: self.delivery_id,
            message: Message {
                seq: self.seq,
                id: self.id,
                target_id: self.target_id,
                author_id: self.author_id,
                client_request_id: self.client_request_id,
                text,
                created_at_ms: self.created_at_ms,
            },
        })
    }
}

/// Column projection for an inbox batch's fenced identity.
struct InboxBatchRow {
    agent_id: String,
    session_id: String,
    generation: i64,
}

/// One active parent member that may be addressed explicitly from a Thread.
struct MentionRecipient {
    id: String,
    kind: String,
    handle: String,
}

impl FromRow for MentionRecipient {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            handle: row.get(2)?,
        })
    }
}

fn is_mention_prefix(character: char) -> bool {
    !character.is_ascii_alphanumeric() && character != '_' && character != '@'
}

fn is_handle_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '-'
}

/// Match one exact `@handle` token without accepting emails or handle prefixes.
fn mentions_handle(text: &str, handle: &str) -> bool {
    let needle = format!("@{handle}");
    text.match_indices(&needle).any(|(start, matched)| {
        let prefix_matches = text[..start]
            .chars()
            .next_back()
            .is_none_or(is_mention_prefix);
        let suffix_matches = text[start + matched.len()..]
            .chars()
            .next()
            .is_none_or(|character| !is_handle_character(character));
        prefix_matches && suffix_matches
    })
}

impl FromRow for InboxBatchRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            agent_id: row.get(0)?,
            session_id: row.get(1)?,
            generation: row.get(2)?,
        })
    }
}

impl<'connection> DeliveryStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Scan the level-triggered wake ledger. Only authorized deliveries that
    /// have not reached a model request contribute to the returned watermark.
    pub(crate) async fn pending_wakes(&self, limit: u32) -> Result<Vec<PendingWake>> {
        self.connection
            .query_rows::<PendingWake>(
                "SELECT rb.agent_id, rb.session_id, rb.generation,
                        rb.provider, rb.model, rb.preset, rb.bound_at_ms,
                        pending.pending_seq
                 FROM runtime_bindings rb
                 JOIN agent_wake_state wake ON wake.agent_id = rb.agent_id
                 JOIN (
                   SELECT d.recipient_id AS agent_id, MAX(d.message_seq) AS pending_seq
                   FROM deliveries d
                   WHERE d.model_seen_at_ms IS NULL
                     AND (
                       EXISTS (
                         SELECT 1 FROM v_target_access access
                         WHERE access.target_id = d.target_id
                           AND access.actor_id = d.recipient_id
                       )
                       OR EXISTS (
                         SELECT 1 FROM targets target
                         WHERE target.id = d.target_id
                           AND target.deleted_at_ms IS NOT NULL
                       )
                     )
                   GROUP BY d.recipient_id
                 ) pending ON pending.agent_id = rb.agent_id
                 WHERE pending.pending_seq > wake.notified_seq
                    OR wake.notified_generation <> rb.generation
                    OR EXISTS (
                      SELECT 1 FROM deliveries d
                      WHERE d.recipient_id = rb.agent_id
                        AND d.model_seen_at_ms IS NULL
                        AND (d.notified_at_ms IS NULL
                          OR d.notified_generation <> rb.generation)
                        AND (
                          EXISTS (
                            SELECT 1 FROM v_target_access access
                            WHERE access.target_id = d.target_id
                              AND access.actor_id = d.recipient_id
                          )
                          OR EXISTS (
                            SELECT 1 FROM targets target
                            WHERE target.id = d.target_id
                              AND target.deleted_at_ms IS NOT NULL
                          )
                        )
                    )
                 ORDER BY pending.pending_seq, rb.agent_id
                 LIMIT ?1",
                [i64::from(limit)],
            )
            .await
    }

    /// The durable pending watermark held by one Agent's wake state.
    pub(crate) async fn durable_pending_seq(&self, agent_id: &str) -> Result<i64> {
        self.connection
            .query_row::<i64>(
                "SELECT pending_seq FROM agent_wake_state WHERE agent_id = ?1",
                [agent_id],
            )
            .await?
            .ok_or_else(|| CollabError::NotFound {
                entity: "wake state",
                id: agent_id.to_owned(),
            })
    }

    /// Record that the exact current Session generation accepted a
    /// content-free wake through `pending_seq`. A concurrently committed
    /// newer Message stays above this watermark and is returned by the next
    /// scan.
    pub(crate) async fn record_notified(
        &self,
        agent_id: &str,
        pending_seq: i64,
        generation: i64,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE agent_wake_state
                 SET notified_seq = MAX(notified_seq, ?2),
                     notified_generation = ?3,
                     attempt_count = 0,
                     next_retry_at_ms = NULL,
                     last_error = NULL
                 WHERE agent_id = ?1",
                (agent_id, pending_seq, generation),
            )
            .await?;
        self.connection
            .execute(
                "UPDATE deliveries
                 SET notified_at_ms = ?3, notified_generation = ?4
                 WHERE recipient_id = ?1
                   AND message_seq <= ?2
                   AND model_seen_at_ms IS NULL
                   AND EXISTS (
                     SELECT 1
                     FROM targets target
                     WHERE target.id = deliveries.target_id
                       AND (
                         target.deleted_at_ms IS NOT NULL
                         OR EXISTS (
                           SELECT 1 FROM v_target_access access
                           WHERE access.target_id = deliveries.target_id
                             AND access.actor_id = deliveries.recipient_id
                         )
                       )
                   )",
                (agent_id, pending_seq, now, generation),
            )
            .await?;
        Ok(())
    }

    /// Re-arm model-unseen work for the same persisted Session. The current
    /// binding still fences the operation; no new generation is created.
    pub(crate) async fn rearm(&self, agent_id: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE agent_wake_state
                 SET notified_generation = 0
                 WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        Ok(())
    }

    /// The authorized, not-yet-model-seen deliveries for one current runtime,
    /// oldest first.
    pub(crate) async fn unseen_deliveries(
        &self,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<InboxMessage>> {
        let rows = self
            .connection
            .query_rows::<InboxMessageRow>(
                "SELECT d.id, m.seq, m.id, m.target_id, m.author_id,
                        m.client_request_id, m.body_json, m.created_at_ms
                 FROM deliveries d
                 JOIN messages m ON m.id = d.message_id
                 WHERE d.recipient_id = ?1
                   AND d.model_seen_at_ms IS NULL
                   AND (
                     EXISTS (
                       SELECT 1 FROM v_target_access access
                       WHERE access.target_id = m.target_id
                         AND access.actor_id = ?1
                     )
                     OR EXISTS (
                       SELECT 1 FROM targets target
                       WHERE target.id = m.target_id
                         AND target.deleted_at_ms IS NOT NULL
                     )
                   )
                 ORDER BY m.seq
                 LIMIT ?2",
                (agent_id, i64::from(limit)),
            )
            .await?;
        rows.into_iter()
            .map(InboxMessageRow::into_message)
            .collect()
    }

    /// Persist one returned inbox batch with its durable check timestamp.
    pub(crate) async fn insert_batch(
        &self,
        batch_id: &str,
        agent_id: &str,
        session_id: &str,
        generation: i64,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO inbox_batches
                 (id, agent_id, session_id, generation, checked_at_ms, model_seen_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                (batch_id, agent_id, session_id, generation, now),
            )
            .await?;
        Ok(())
    }

    /// Attach one returned delivery to a batch and stamp it checked once.
    pub(crate) async fn record_batch_item(
        &self,
        batch_id: &str,
        delivery_id: &str,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO inbox_batch_items (batch_id, delivery_id) VALUES (?1, ?2)",
                (batch_id, delivery_id),
            )
            .await?;
        self.connection
            .execute(
                "UPDATE deliveries
                 SET checked_at_ms = COALESCE(checked_at_ms, ?2)
                 WHERE id = ?1",
                (delivery_id, now),
            )
            .await?;
        Ok(())
    }

    /// Fail unless one persisted batch belongs exactly to the fenced Agent,
    /// Session, and generation.
    pub(crate) async fn require_batch(
        &self,
        batch_id: &str,
        agent_id: &str,
        session_id: &str,
        generation: i64,
    ) -> Result<()> {
        let InboxBatchRow {
            agent_id: batch_agent,
            session_id: batch_session,
            generation: batch_generation,
        } = self
            .connection
            .query_row::<InboxBatchRow>(
                "SELECT agent_id, session_id, generation
                 FROM inbox_batches WHERE id = ?1",
                [batch_id],
            )
            .await?
            .ok_or_else(|| CollabError::NotFound {
                entity: "inbox batch",
                id: batch_id.to_owned(),
            })?;
        if batch_agent != agent_id || batch_session != session_id || batch_generation != generation
        {
            return Err(CollabError::RuntimeGenerationMismatch {
                agent_id: agent_id.to_owned(),
            });
        }
        Ok(())
    }

    /// Stamp one batch's deliveries and the batch row itself model-seen once.
    pub(crate) async fn mark_seen(
        &self,
        batch_id: &str,
        now: i64,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE deliveries
                 SET model_seen_at_ms = COALESCE(model_seen_at_ms, ?2),
                     seen_generation = COALESCE(seen_generation, ?3),
                     seen_session_id = COALESCE(seen_session_id, ?4)
                 WHERE id IN (
                   SELECT delivery_id FROM inbox_batch_items WHERE batch_id = ?1
                 )",
                (batch_id, now, generation, session_id),
            )
            .await?;
        self.connection
            .execute(
                "UPDATE inbox_batches
                 SET model_seen_at_ms = COALESCE(model_seen_at_ms, ?2)
                 WHERE id = ?1",
                (batch_id, now),
            )
            .await?;
        Ok(())
    }
}

impl DeliveryStore<'_> {
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

    /// Active parent members explicitly addressed by an exact `@handle` token.
    pub(crate) async fn active_thread_mention_recipients(
        &self,
        permission_target_id: &str,
        author_id: &str,
        text: &str,
    ) -> Result<Vec<Recipient>> {
        if !text.contains('@') {
            return Ok(Vec::new());
        }
        let candidates = self
            .connection
            .query_rows::<MentionRecipient>(
                "SELECT a.id, a.kind, a.handle
                 FROM memberships m
                 JOIN actors a ON a.id = m.actor_id
                 WHERE m.target_id = ?1
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                (permission_target_id, author_id),
            )
            .await?;
        Ok(candidates
            .into_iter()
            .filter(|candidate| mentions_handle(text, &candidate.handle))
            .map(|candidate| Recipient {
                id: candidate.id,
                kind: candidate.kind,
            })
            .collect())
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

impl DeliveryStore<'_> {
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
}

#[cfg(test)]
mod mention_tests {
    use super::mentions_handle;

    #[test]
    fn exact_mentions_respect_token_boundaries() {
        assert!(mentions_handle("@alpha please review", "alpha"));
        assert!(mentions_handle("请看一下，@alpha。", "alpha"));
        assert!(!mentions_handle("mail@alpha.example", "alpha"));
        assert!(!mentions_handle("@alpha-two", "alpha"));
        assert!(!mentions_handle("@alpha中文", "alpha"));
        assert!(mentions_handle(
            "请 @grok老马melody 看一下",
            "grok老马melody"
        ));
        assert!(!mentions_handle("@grok老马melody扩展", "grok老马melody"));
        assert!(!mentions_handle("@@alpha", "alpha"));
    }
}
