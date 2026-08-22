//! Level-triggered wake delivery, inbox batches, and model-seen fences.

use super::*;

impl CollabCore {
    /// Scan the level-triggered wake ledger. Only authorized deliveries that
    /// have not reached a model request contribute to the returned watermark.
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<PendingWake>> {
        self.assert_open()?;
        if limit == 0 || limit > 1000 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 1000".into(),
            ));
        }
        let connection = self.connection.lock().await;
        let mut rows = connection
            .query(
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
            .await?;
        let mut wakes = Vec::new();
        while let Some(row) = rows.next().await? {
            wakes.push(PendingWake {
                binding: RuntimeBinding {
                    agent_id: row.get(0)?,
                    session_id: row.get(1)?,
                    generation: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    preset: row.get(5)?,
                    bound_at_ms: row.get(6)?,
                },
                pending_seq: row.get(7)?,
            });
        }
        Ok(wakes)
    }

    /// Record that the exact current Session generation accepted a content-free
    /// wake through `pending_seq`. A concurrently committed newer Message stays
    /// above this watermark and will be returned by the next scan.
    pub async fn mark_notified(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        pending_seq: i64,
    ) -> Result<()> {
        self.assert_open()?;
        if pending_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "pending_seq must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
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
        let durable_pending = row.get::<i64>(0)?;
        drop(rows);
        if pending_seq > durable_pending {
            return Err(CollabError::InvalidArgument(format!(
                "pending_seq {pending_seq} exceeds durable watermark {durable_pending}"
            )));
        }

        transaction
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
        transaction
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
        transaction.commit().await?;
        Ok(())
    }

    /// Re-arm model-unseen work when the same persisted Session is resumed into
    /// a fresh process-local Agent inbox. This does not create a new Session
    /// generation; the current binding still fences the operation.
    pub async fn rearm_runtime_wake(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        self.assert_open()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;
        transaction
            .execute(
                "UPDATE agent_wake_state
                 SET notified_generation = 0
                 WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Return the authorized, not-yet-model-seen deliveries for the current
    /// runtime generation and persist the exact returned batch.
    pub async fn check_inbox(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        limit: u32,
    ) -> Result<InboxBatch> {
        self.assert_open()?;
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
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
            let body: StoredTextBody = serde_json::from_str(&body_json).map_err(|error| {
                CollabError::Database(format!("message body is malformed: {error}"))
            })?;
            messages.push(InboxMessage {
                delivery_id,
                message: Message {
                    seq: row.get(1)?,
                    id: row.get(2)?,
                    target_id: row.get(3)?,
                    author_id: row.get(4)?,
                    client_request_id: row.get(5)?,
                    text: body.text,
                    created_at_ms: row.get(7)?,
                },
            });
        }
        drop(rows);

        let mut seen_target_ids = BTreeSet::new();
        let mut contexts = Vec::new();
        for item in &messages {
            let target_id = item.message.target_id.as_str();
            if seen_target_ids.insert(target_id.to_owned()) {
                contexts.push(identity_context_for(&transaction, agent_id, Some(target_id)).await?);
            }
        }

        if messages.is_empty() {
            transaction.commit().await?;
            return Ok(InboxBatch {
                id: None,
                agent_id: agent_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
                messages,
                contexts,
                checked_at_ms: now,
            });
        }

        let batch_id = new_id();
        transaction
            .execute(
                "INSERT INTO inbox_batches
                 (id, agent_id, session_id, generation, checked_at_ms, model_seen_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                (batch_id.as_str(), agent_id, session_id, generation, now),
            )
            .await?;
        for item in &messages {
            transaction
                .execute(
                    "INSERT INTO inbox_batch_items (batch_id, delivery_id) VALUES (?1, ?2)",
                    (batch_id.as_str(), item.delivery_id.as_str()),
                )
                .await?;
            transaction
                .execute(
                    "UPDATE deliveries
                     SET checked_at_ms = COALESCE(checked_at_ms, ?2)
                     WHERE id = ?1",
                    (item.delivery_id.as_str(), now),
                )
                .await?;
        }
        transaction.commit().await?;

        Ok(InboxBatch {
            id: Some(batch_id),
            agent_id: agent_id.to_owned(),
            session_id: session_id.to_owned(),
            generation,
            messages,
            contexts,
            checked_at_ms: now,
        })
    }

    /// Mark one check batch model-seen only if its Session generation is still
    /// the current runtime binding.
    pub async fn mark_model_seen(
        &self,
        batch_id: &str,
        agent_id: &str,
        generation: i64,
        session_id: &str,
    ) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_current_binding(&transaction, agent_id, generation, session_id).await?;

        let mut rows = transaction
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
        drop(rows);
        if batch_agent != agent_id || batch_session != session_id || batch_generation != generation
        {
            return Err(CollabError::RuntimeGenerationMismatch {
                agent_id: agent_id.to_owned(),
            });
        }

        transaction
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
        transaction
            .execute(
                "UPDATE inbox_batches
                 SET model_seen_at_ms = COALESCE(model_seen_at_ms, ?2)
                 WHERE id = ?1",
                (batch_id, now),
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn runtime_generation_fences_model_seen_receipts() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let first_binding = core
            .bind_runtime(&alpha.id, "session-1", "openai", "codex", "default")
            .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "generation-message".into(),
            text: "read me".into(),
        })
        .await?;
        let batch = core
            .check_inbox(
                &alpha.id,
                first_binding.generation,
                &first_binding.session_id,
                10,
            )
            .await?;
        let batch_id = batch.id.expect("batch with one message");
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.contexts.len(), 1);
        assert_eq!(batch.contexts[0].agent.actor.id, alpha.id);
        assert_eq!(batch.contexts[0].target.as_ref(), Some(&channel));
        assert!(
            batch.contexts[0]
                .members
                .iter()
                .any(|member| member.actor.id == user.id && member.role == MembershipRole::Owner)
        );

        let second_binding = core
            .bind_runtime(&alpha.id, "session-2", "openai", "codex", "default")
            .await?;
        assert_eq!(second_binding.generation, first_binding.generation + 1);
        let stale = core
            .mark_model_seen(
                &batch_id,
                &alpha.id,
                first_binding.generation,
                &first_binding.session_id,
            )
            .await;
        assert!(matches!(
            stale,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));

        let redelivered = core
            .check_inbox(
                &alpha.id,
                second_binding.generation,
                &second_binding.session_id,
                10,
            )
            .await?;
        assert_eq!(redelivered.messages.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn wake_watermark_is_level_triggered_and_generation_fenced() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let first = core
            .bind_runtime(&alpha.id, "wake-session-1", "openai", "codex", "default")
            .await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "wake-message".into(),
                text: "ring once".into(),
            })
            .await?;

        let wakes = core.list_pending_wakes(10).await?;
        assert_eq!(wakes.len(), 1);
        assert_eq!(wakes[0].binding, first);
        assert_eq!(wakes[0].pending_seq, sent.message.seq);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        core.rearm_runtime_wake(&alpha.id, first.generation, &first.session_id)
            .await?;
        assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        {
            let connection = core.connection.lock().await;
            connection
                .execute(
                    "UPDATE memberships SET left_at_ms = 1
                         WHERE target_id = ?1 AND actor_id = ?2",
                    (channel.id.as_str(), alpha.id.as_str()),
                )
                .await?;
        }
        assert!(core.list_pending_wakes(10).await?.is_empty());
        core.add_member(&channel.id, &alpha.id, &user.id).await?;
        assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());

        let second = core
            .bind_runtime(&alpha.id, "wake-session-2", "openai", "codex", "default")
            .await?;
        let rebound = core.list_pending_wakes(10).await?;
        assert_eq!(rebound.len(), 1);
        assert_eq!(rebound[0].binding, second);
        assert!(matches!(
            core.mark_notified(
                &alpha.id,
                first.generation,
                &first.session_id,
                sent.message.seq,
            )
            .await,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));
        core.mark_notified(
            &alpha.id,
            second.generation,
            &second.session_id,
            sent.message.seq,
        )
        .await?;

        let batch = core
            .check_inbox(&alpha.id, second.generation, &second.session_id, 10)
            .await?;
        core.mark_model_seen(
            batch.id.as_deref().expect("one-message batch"),
            &alpha.id,
            second.generation,
            &second.session_id,
        )
        .await?;
        core.bind_runtime(&alpha.id, "wake-session-3", "openai", "codex", "default")
            .await?;
        assert!(core.list_pending_wakes(10).await?.is_empty());
        Ok(())
    }
}
