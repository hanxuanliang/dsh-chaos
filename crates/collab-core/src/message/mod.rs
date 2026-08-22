//! Immutable Message writes, recipient snapshots, and authorized history reads.

mod model;
pub(crate) mod store;

pub use model::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub(crate) use model::{NewMessage, Recipient, StoredTextBody, stored_text};

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::target::AccessGrant;
use crate::thread::ThreadId;
use crate::thread::store::ThreadStore;
use crate::{ChangeKind, CollabCore, CollabError, NonBlank, Result, TargetKind, new_id, now_ms};

use model::SendFailpoint;
use store::MessageStore;

#[cfg(test)]
pub(crate) use model::SendFailpoint as TestSendFailpoint;

impl CollabCore {
    /// Atomically commit one immutable Message, its recipient snapshot, and
    /// every recipient Agent's level-triggered wake watermark.
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<SendMessageResult> {
        self.send_message_inner(request, SendFailpoint::None).await
    }

    pub(crate) async fn send_message_inner(
        &self,
        request: SendMessageRequest,
        _failpoint: SendFailpoint,
    ) -> Result<SendMessageResult> {
        NonBlank::parse("target_id", &request.target_id)?;
        NonBlank::parse("author_id", &request.author_id)?;
        NonBlank::parse("client_request_id", &request.client_request_id)?;
        NonBlank::parse("text", &request.text)?;

        let now = now_ms()?;
        self.write(async |connection| {
            let route = AccessGrant::require(connection, &request.target_id, &request.author_id)
                .await?
                .route;
            let store = MessageStore::new(connection);

            if let Some(message) = store
                .find_by_request(&request.author_id, &request.client_request_id)
                .await?
            {
                let (recipient_ids, wake_agent_ids) =
                    crate::delivery::store::DeliveryStore::new(connection)
                        .recorded_recipients(&message.id)
                        .await?;
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

            let delivery = crate::delivery::store::DeliveryStore::new(connection);
            let recipients = if route.kind == TargetKind::Thread {
                delivery
                    .active_thread_recipients(
                        &request.target_id,
                        &request.author_id,
                        route.permission_target_id(&request.target_id),
                    )
                    .await?
            } else {
                delivery
                    .active_target_recipients(&request.target_id, &request.author_id)
                    .await?
            };
            let (recipient_ids, wake_agent_ids) = delivery
                .record_deliveries(&draft, message_seq, &recipients)
                .await?;
            ChangeStore::new(connection)
                .insert_target_change(
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

impl CollabCore {
    /// Read one exact Message while the actor retains access to the exact
    /// target, inherited from the parent for a Thread.
    pub async fn read_message(
        &self,
        actor_id: &str,
        target_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        self.read(async |connection| {
            AccessGrant::require(connection, target_id, actor_id).await?;
            MessageStore::new(connection)
                .require_in_target(target_id, message_id)
                .await
        })
        .await
    }

    /// Read an ascending page from one exact target after a global sequence.
    pub async fn read_messages(
        &self,
        actor_id: &str,
        target_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        self.read(async |connection| {
            AccessGrant::require(connection, target_id, actor_id).await?;
            MessageStore::new(connection)
                .page_after(target_id, after_seq, limit)
                .await
        })
        .await
    }

    /// Read the exact total count and the latest `limit` messages (ascending)
    /// of one exact target. Both reads share one Deferred snapshot, so
    /// `count` is never a lower bound and the page is always the true tail,
    /// even under concurrent writers on another connection.
    pub async fn read_messages_tail(
        &self,
        actor_id: &str,
        target_id: &str,
        limit: u32,
    ) -> Result<MessageTail> {
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        self.read(async |connection| {
            AccessGrant::require(connection, target_id, actor_id).await?;
            let store = MessageStore::new(connection);
            let count = store.count_in_target(target_id).await?;
            let messages = store.tail(target_id, limit).await?;
            Ok(MessageTail { count, messages })
        })
        .await
    }
}
