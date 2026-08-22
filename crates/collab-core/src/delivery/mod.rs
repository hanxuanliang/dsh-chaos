//! Level-triggered wake delivery, inbox batches, and model-seen fences.

mod model;
pub(crate) mod store;

pub use model::{InboxBatch, InboxMessage, PendingWake};

use std::collections::BTreeSet;

use crate::membership::identity_context_for;
use crate::runtime::require_current_binding;
use crate::{CollabCore, CollabError, Result, new_id, now_ms};

use store::DeliveryStore;

impl CollabCore {
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
        if pending_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "pending_seq must be positive".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            require_current_binding(connection, agent_id, generation, session_id).await?;
            let store = DeliveryStore::new(connection);
            let durable_pending = store.durable_pending_seq(agent_id).await?;
            if pending_seq > durable_pending {
                return Err(CollabError::InvalidArgument(format!(
                    "pending_seq {pending_seq} exceeds durable watermark {durable_pending}"
                )));
            }
            store
                .record_notified(agent_id, pending_seq, generation, now)
                .await
        })
        .await
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
        self.write(async |connection| {
            require_current_binding(connection, agent_id, generation, session_id).await?;
            DeliveryStore::new(connection).rearm(agent_id).await
        })
        .await
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
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            require_current_binding(connection, agent_id, generation, session_id).await?;
            let store = DeliveryStore::new(connection);
            let messages = store.unseen_deliveries(agent_id, limit).await?;

            let mut seen_target_ids = BTreeSet::new();
            let mut contexts = Vec::new();
            for item in &messages {
                let target_id = item.message.target_id.as_str();
                if seen_target_ids.insert(target_id.to_owned()) {
                    contexts
                        .push(identity_context_for(connection, agent_id, Some(target_id)).await?);
                }
            }

            if messages.is_empty() {
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
            store
                .insert_batch(&batch_id, agent_id, session_id, generation, now)
                .await?;
            for item in &messages {
                store
                    .record_batch_item(&batch_id, &item.delivery_id, now)
                    .await?;
            }
            Ok(InboxBatch {
                id: Some(batch_id),
                agent_id: agent_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
                messages,
                contexts,
                checked_at_ms: now,
            })
        })
        .await
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
        let now = now_ms()?;
        self.write(async |connection| {
            require_current_binding(connection, agent_id, generation, session_id).await?;
            let store = DeliveryStore::new(connection);
            store
                .require_batch(batch_id, agent_id, session_id, generation)
                .await?;
            store.mark_seen(batch_id, now, generation, session_id).await
        })
        .await
    }
}

impl CollabCore {
    /// Scan the level-triggered wake ledger. Only authorized deliveries that
    /// have not reached a model request contribute to the returned watermark.
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<PendingWake>> {
        if limit == 0 || limit > 1000 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 1000".into(),
            ));
        }
        self.read(async |connection| DeliveryStore::new(connection).pending_wakes(limit).await)
            .await
    }
}
