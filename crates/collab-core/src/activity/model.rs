use uuid::Uuid;

use crate::{ActivityInboxItem, CollabError, Result};

/// Keyset cursor over the Activity inbox: `<sequence>:<conversation-id>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActivityCursor {
    pub(crate) last_activity_seq: i64,
    pub(crate) conversation_id: String,
}

impl ActivityCursor {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let Some((sequence, conversation_id)) = value.split_once(':') else {
            return Err(CollabError::InvalidArgument(
                "cursor must be '<sequence>:<target-id>'".into(),
            ));
        };
        let last_activity_seq = sequence.parse::<i64>().map_err(|_| {
            CollabError::InvalidArgument("cursor sequence must be a positive integer".into())
        })?;
        if last_activity_seq <= 0
            || conversation_id.is_empty()
            || conversation_id.contains(':')
            || Uuid::parse_str(conversation_id).is_err()
        {
            return Err(CollabError::InvalidArgument(
                "cursor must be '<positive-sequence>:<target-id>'".into(),
            ));
        }
        Ok(Self {
            last_activity_seq,
            conversation_id: conversation_id.to_owned(),
        })
    }

    pub(crate) fn for_item(item: &ActivityInboxItem) -> String {
        format!("{}:{}", item.last_activity_seq, item.conversation_id)
    }
}
