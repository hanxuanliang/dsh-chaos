use serde::{Deserialize, Serialize};
use turso::Connection;

use crate::actor::ActorId;
use crate::membership::Membership;
use crate::target::TargetRoute;
use crate::{Actor, CollabError, Result, TargetKind};

string_id!(ThreadId, "thread_target_id");

/// Batch Thread preview for one root Message: count plus recent distinct repliers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadSummary {
    pub root_message_id: String,
    pub thread_id: String,
    pub reply_count: i64,
    pub last_reply_at_ms: Option<i64>,
    pub recent_replier_ids: Vec<String>,
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
            if !unique.contains(value) {
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

/// A `TargetRoute` proven to describe an active Thread, with the permission
/// target resolved once at construction.
#[derive(Clone, Debug)]
pub(crate) struct ThreadRoute {
    permission_target_id: String,
}

impl ThreadRoute {
    /// Certify that `thread_id` is an active Thread and resolve the target
    /// whose membership governs it.
    pub(crate) async fn require(connection: &Connection, thread_id: &ThreadId) -> Result<Self> {
        let route = TargetRoute::require(connection, thread_id.as_str()).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "expected a Thread target".into(),
            ));
        }
        Ok(Self {
            permission_target_id: route.permission_target_id(thread_id.as_str()).to_owned(),
        })
    }

    pub(crate) fn permission_target_id(&self) -> &str {
        &self.permission_target_id
    }

    fn into_permission_target_id(self) -> String {
        self.permission_target_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThreadAccess {
    pub(crate) thread_id: ThreadId,
    pub(crate) permission_target_id: String,
}

impl ThreadAccess {
    /// Certify that one Actor may act in one Thread: the actor exists, the
    /// target is an active Thread, and the actor is an active member of the
    /// Thread's permission target.
    pub(crate) async fn require(
        connection: &Connection,
        actor_id: &ActorId,
        thread_id: &ThreadId,
    ) -> Result<Self> {
        let actor = Actor::require(connection, actor_id).await?;
        let route = ThreadRoute::require(connection, thread_id).await?;
        Membership::require(connection, route.permission_target_id(), &actor).await?;
        Ok(Self {
            thread_id: thread_id.clone(),
            permission_target_id: route.into_permission_target_id(),
        })
    }
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
