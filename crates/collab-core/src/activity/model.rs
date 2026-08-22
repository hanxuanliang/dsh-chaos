use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ActorKind, CollabError, Result, TargetKind, TaskStatus};

/// Which conversations the Activity inbox lists: those with unseen activity,
/// or every conversation including done ones.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityFilter {
    #[default]
    Unread,
    All,
}

impl ActivityFilter {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("unread") => Ok(Self::Unread),
            Some("all") => Ok(Self::All),
            Some(other) => Err(CollabError::InvalidArgument(format!(
                "filter must be 'unread' or 'all', got '{other}'"
            ))),
        }
    }
}

/// The title source used by one Activity inbox row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTitleKind {
    Thread,
    Message,
}

#[cfg(feature = "napi")]
impl ActivityTitleKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Message => "message",
        }
    }
}

/// Latest Message preview joined into one Activity inbox row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxReply {
    pub sender_name: String,
    pub sender_kind: ActorKind,
    pub excerpt: String,
    pub at_ms: i64,
}

/// Compact Task state associated with the row's anchor Message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxTask {
    pub number: i64,
    pub status: TaskStatus,
    pub assignee_name: Option<String>,
}

/// One active Channel, Direct, or followed Thread conversation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxItem {
    pub conversation_id: String,
    pub target_kind: TargetKind,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub target_name: String,
    pub title_kind: ActivityTitleKind,
    pub title: String,
    pub latest_reply: Option<ActivityInboxReply>,
    pub last_activity_at_ms: i64,
    pub last_activity_seq: i64,
    pub reply_count: Option<i64>,
    pub task: Option<ActivityInboxTask>,
    /// Whether the actor's Done fence already covers the latest activity.
    /// Always false under the Unread filter.
    pub done: bool,
}

/// One newest-first Activity page plus the total active conversation count.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxPage {
    pub items: Vec<ActivityInboxItem>,
    pub next_cursor: Option<String>,
    pub active_count: i64,
}

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
