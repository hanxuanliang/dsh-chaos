use crate::db::FromRow;
use crate::ids::{ActorId, ThreadId};
use crate::{CollabError, Result};
use serde::{Deserialize, Serialize};
use turso::Row;

/// Batch Thread preview for one root Message: count plus recent distinct repliers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadSummary {
    pub root_message_id: String,
    pub thread_id: String,
    pub reply_count: i64,
    pub last_reply_at_ms: Option<i64>,
    pub recent_replier_ids: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ThreadCountRow {
    pub(crate) thread_id: String,
    pub(crate) root_message_id: String,
    pub(crate) reply_count: i64,
    pub(crate) last_reply_at_ms: Option<i64>,
}

impl FromRow for ThreadCountRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            thread_id: row.get(0)?,
            root_message_id: row.get(1)?,
            reply_count: row.get(2)?,
            last_reply_at_ms: row.get(3)?,
        })
    }
}

impl ThreadCountRow {
    pub(crate) fn into_summary(self) -> ThreadSummary {
        ThreadSummary {
            root_message_id: self.root_message_id,
            thread_id: self.thread_id,
            reply_count: self.reply_count,
            last_reply_at_ms: self.last_reply_at_ms,
            recent_replier_ids: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ThreadReplierRow {
    pub(crate) thread_id: String,
    pub(crate) author_id: String,
}

impl FromRow for ThreadReplierRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            thread_id: row.get(0)?,
            author_id: row.get(1)?,
        })
    }
}

#[derive(Debug)]
pub(crate) struct FollowedThreadId(pub(crate) String);

impl FromRow for FollowedThreadId {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RootMessageIds(Vec<String>);

impl RootMessageIds {
    pub(crate) fn parse(values: &[String], maximum: usize) -> Result<Self> {
        if values.len() > maximum {
            return Err(CollabError::InvalidArgument(format!(
                "root_message_ids must contain at most {maximum} ids"
            )));
        }
        let mut unique = Vec::new();
        for value in values {
            if value.len() > 64
                || !value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
            {
                return Err(CollabError::InvalidArgument(
                    "root_message_id must be a backend-assigned id".into(),
                ));
            }
            if !unique.iter().any(|existing| existing == value) {
                unique.push(value.clone());
            }
        }
        Ok(Self(unique))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_slice(&self) -> &[String] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FollowState {
    Following,
    NotFollowing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FollowOutcome {
    Changed,
    Unchanged,
}

impl FollowOutcome {
    pub(crate) const fn changed(self) -> bool {
        matches!(self, Self::Changed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThreadSubscription {
    thread_id: ThreadId,
    actor_id: ActorId,
    state: FollowState,
}

impl ThreadSubscription {
    pub(crate) fn new(thread_id: ThreadId, actor_id: ActorId, state: FollowState) -> Self {
        Self {
            thread_id,
            actor_id,
            state,
        }
    }

    pub(crate) fn thread_id(&self) -> &ThreadId {
        &self.thread_id
    }

    pub(crate) fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    pub(crate) const fn state(&self) -> FollowState {
        self.state
    }

    pub(crate) fn follow(&mut self) -> FollowOutcome {
        match self.state {
            FollowState::Following => FollowOutcome::Unchanged,
            FollowState::NotFollowing => {
                self.state = FollowState::Following;
                FollowOutcome::Changed
            }
        }
    }

    pub(crate) fn unfollow(&mut self) -> FollowOutcome {
        match self.state {
            FollowState::Following => {
                self.state = FollowState::NotFollowing;
                FollowOutcome::Changed
            }
            FollowState::NotFollowing => FollowOutcome::Unchanged,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThreadAccess {
    pub(crate) thread_id: ThreadId,
    pub(crate) permission_target_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_transitions_are_idempotent() {
        let mut subscription = ThreadSubscription::new(
            ThreadId::parse("thread").expect("valid Thread id"),
            ActorId::parse("actor").expect("valid Actor id"),
            FollowState::NotFollowing,
        );

        assert_eq!(subscription.follow(), FollowOutcome::Changed);
        assert_eq!(subscription.follow(), FollowOutcome::Unchanged);
        assert_eq!(subscription.unfollow(), FollowOutcome::Changed);
        assert_eq!(subscription.unfollow(), FollowOutcome::Unchanged);
    }
}
