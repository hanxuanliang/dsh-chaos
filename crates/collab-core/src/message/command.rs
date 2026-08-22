use crate::changefeed::insert_target_change;
use crate::ids::{ActorId, ThreadId};
use crate::target::require_target_access;
use crate::thread::store::ThreadStore;
use crate::{
    ChangeKind, CollabCore, CollabError, Message, Result, SendMessageRequest, SendMessageResult,
    TargetKind, new_id, now_ms, require_non_empty,
};

use super::model::{NewMessage, SendFailpoint, StoredTextBody};
use super::store::MessageStore;

impl CollabCore {
    /// Atomically commit one immutable Message, its recipient snapshot, and
    /// every recipient Agent's level-triggered wake watermark.
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<SendMessageResult> {
        self.send_message_inner(request, SendFailpoint::None).await
    }

    pub(super) async fn send_message_inner(
        &self,
        request: SendMessageRequest,
        _failpoint: SendFailpoint,
    ) -> Result<SendMessageResult> {
        for (name, value) in [
            ("target_id", request.target_id.as_str()),
            ("author_id", request.author_id.as_str()),
            ("client_request_id", request.client_request_id.as_str()),
            ("text", request.text.as_str()),
        ] {
            require_non_empty(name, value)?;
        }

        let now = now_ms()?;
        self.write(async |connection| {
            let route =
                require_target_access(connection, &request.target_id, &request.author_id).await?;
            let store = MessageStore::new(connection);

            if let Some(message) = store
                .find_by_request(&request.author_id, &request.client_request_id)
                .await?
            {
                let (recipient_ids, wake_agent_ids) =
                    store.recorded_recipients(&message.id).await?;
                return Ok(SendMessageResult {
                    message,
                    recipient_ids,
                    wake_agent_ids,
                    replayed: true,
                });
            }

            if route.kind == TargetKind::Thread {
                ThreadStore::new(connection)
                    .ensure_following(
                        &ThreadId::parse(&request.target_id)?,
                        &ActorId::parse(&request.author_id)?,
                        now,
                    )
                    .await?;
            }

            let body_json = serde_json::to_string(&StoredTextBody {
                kind: "text".into(),
                text: request.text.clone(),
            })
            .map_err(|error| CollabError::InvalidArgument(error.to_string()))?;
            let draft = NewMessage {
                id: new_id(),
                target_id: request.target_id.clone(),
                author_id: request.author_id.clone(),
                client_request_id: request.client_request_id.clone(),
                body_json,
                created_at_ms: now,
            };
            let message_seq = store.insert(&draft).await?;

            #[cfg(test)]
            if _failpoint == SendFailpoint::AfterMessageInsert {
                return Err(CollabError::InjectedSendFailure);
            }

            let recipients = if route.kind == TargetKind::Thread {
                store
                    .active_thread_recipients(
                        &request.target_id,
                        &request.author_id,
                        route.permission_target_id(&request.target_id),
                    )
                    .await?
            } else {
                store
                    .active_target_recipients(&request.target_id, &request.author_id)
                    .await?
            };
            let (recipient_ids, wake_agent_ids) = store
                .record_deliveries(&draft, message_seq, &recipients)
                .await?;
            insert_target_change(
                connection,
                ChangeKind::MessageCreated,
                &request.target_id,
                &draft.id,
                &[request.author_id.as_str()],
                now,
            )
            .await?;

            Ok(SendMessageResult {
                message: Message {
                    seq: message_seq,
                    id: draft.id,
                    target_id: request.target_id,
                    author_id: request.author_id,
                    client_request_id: request.client_request_id,
                    text: request.text,
                    created_at_ms: now,
                },
                recipient_ids,
                wake_agent_ids,
                replayed: false,
            })
        })
        .await
    }
}
