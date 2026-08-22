//! NAPI bridge for the delivery vertical.

use crate::{InboxBatch, PendingWake};
use napi::Result;
use napi_derive::napi;

use super::membership::JsIdentityContext;
use super::message::JsMessage;
use super::runtime::JsRuntimeBinding;
use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct JsPendingWake {
    pub binding: JsRuntimeBinding,
    pub pending_seq: String,
}

impl From<PendingWake> for JsPendingWake {
    fn from(wake: PendingWake) -> Self {
        Self {
            binding: wake.binding.into(),
            pending_seq: wake.pending_seq.to_string(),
        }
    }
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
    pub contexts: Vec<JsIdentityContext>,
    pub checked_at_ms: f64,
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
            contexts: batch
                .contexts
                .into_iter()
                .map(JsIdentityContext::from)
                .collect(),
            checked_at_ms: batch.checked_at_ms as f64,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<JsPendingWake>> {
        self.core
            .list_pending_wakes(limit)
            .await
            .map(|wakes| wakes.into_iter().map(JsPendingWake::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn mark_notified(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
        pending_seq: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        let pending_seq = parse_i64("pending_seq", &pending_seq)?;
        self.core
            .mark_notified(&agent_id, generation, &session_id, pending_seq)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn rearm_runtime_wake(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .rearm_runtime_wake(&agent_id, generation, &session_id)
            .await
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
}
