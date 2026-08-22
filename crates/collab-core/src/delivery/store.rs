//! Wake ledger, inbox batch, and model-seen persistence.

use turso::Connection;

use crate::db::QueryRows;
use crate::message::stored_text;
use crate::{CollabError, InboxMessage, Message, PendingWake, Result};

/// The authorized, not-yet-model-seen delivery ledger for one Agent.
pub(crate) struct DeliveryStore<'connection> {
    connection: &'connection Connection,
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
                   JOIN targets target ON target.id = d.target_id
                   JOIN memberships mem
                     ON mem.target_id = CASE
                       WHEN target.kind = 'thread' THEN target.parent_target_id
                       ELSE target.id
                     END
                    AND mem.actor_id = d.recipient_id
                    AND mem.left_at_ms IS NULL
                   WHERE d.model_seen_at_ms IS NULL
                   GROUP BY d.recipient_id
                 ) pending ON pending.agent_id = rb.agent_id
                 WHERE pending.pending_seq > wake.notified_seq
                    OR wake.notified_generation <> rb.generation
                    OR EXISTS (
                      SELECT 1 FROM deliveries d
                      JOIN targets target ON target.id = d.target_id
                      JOIN memberships mem
                        ON mem.target_id = CASE
                          WHEN target.kind = 'thread' THEN target.parent_target_id
                          ELSE target.id
                        END
                       AND mem.actor_id = d.recipient_id
                       AND mem.left_at_ms IS NULL
                      WHERE d.recipient_id = rb.agent_id
                        AND d.model_seen_at_ms IS NULL
                        AND (d.notified_at_ms IS NULL
                          OR d.notified_generation <> rb.generation)
                    )
                 ORDER BY pending.pending_seq, rb.agent_id
                 LIMIT ?1",
                [i64::from(limit)],
            )
            .await
    }

    /// The durable pending watermark held by one Agent's wake state.
    pub(crate) async fn durable_pending_seq(&self, agent_id: &str) -> Result<i64> {
        let mut rows = self
            .connection
            .query(
                "SELECT pending_seq FROM agent_wake_state WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "wake state",
                id: agent_id.to_owned(),
            });
        };
        Ok(row.get::<i64>(0)?)
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
                     SELECT 1 FROM targets target
                     JOIN memberships mem
                       ON mem.target_id = CASE
                         WHEN target.kind = 'thread' THEN target.parent_target_id
                         ELSE target.id
                       END
                     WHERE target.id = deliveries.target_id
                       AND mem.actor_id = deliveries.recipient_id
                       AND mem.left_at_ms IS NULL
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
        let mut rows = self
            .connection
            .query(
                "SELECT d.id, m.seq, m.id, m.target_id, m.author_id,
                        m.client_request_id, m.body_json, m.created_at_ms
                 FROM deliveries d
                 JOIN messages m ON m.id = d.message_id
                 JOIN targets target ON target.id = m.target_id
                 JOIN memberships mem
                   ON mem.target_id = CASE
                     WHEN target.kind = 'thread' THEN target.parent_target_id
                     ELSE target.id
                   END
                  AND mem.actor_id = ?1
                 WHERE d.recipient_id = ?1
                   AND d.model_seen_at_ms IS NULL
                   AND mem.left_at_ms IS NULL
                 ORDER BY m.seq
                 LIMIT ?2",
                (agent_id, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            let delivery_id = row.get::<String>(0)?;
            let body_json = row.get::<String>(6)?;
            messages.push(InboxMessage {
                delivery_id,
                message: Message {
                    seq: row.get(1)?,
                    id: row.get(2)?,
                    target_id: row.get(3)?,
                    author_id: row.get(4)?,
                    client_request_id: row.get(5)?,
                    text: stored_text(&body_json, "message body")?,
                    created_at_ms: row.get(7)?,
                },
            });
        }
        Ok(messages)
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
        let mut rows = self
            .connection
            .query(
                "SELECT agent_id, session_id, generation
                 FROM inbox_batches WHERE id = ?1",
                [batch_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "inbox batch",
                id: batch_id.to_owned(),
            });
        };
        let batch_agent = row.get::<String>(0)?;
        let batch_session = row.get::<String>(1)?;
        let batch_generation = row.get::<i64>(2)?;
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
