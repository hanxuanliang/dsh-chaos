use crate::target::require_target_access;
use crate::{CollabCore, CollabError, Message, MessageTail, Result};

use super::store::MessageStore;

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
            require_target_access(connection, target_id, actor_id).await?;
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
            require_target_access(connection, target_id, actor_id).await?;
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
            require_target_access(connection, target_id, actor_id).await?;
            let store = MessageStore::new(connection);
            let count = store.count_in_target(target_id).await?;
            let messages = store.tail(target_id, limit).await?;
            Ok(MessageTail { count, messages })
        })
        .await
    }
}
