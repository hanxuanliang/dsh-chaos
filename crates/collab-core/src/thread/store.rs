use turso::Connection;

use crate::actor::require_actor;
use crate::ids::{ActorId, ThreadId};
use crate::target::{require_active_member, require_target_route};
use crate::{CollabError, Result, TargetKind};

use super::model::{FollowOutcome, FollowState, ThreadAccess, ThreadSubscription};

pub(crate) struct ThreadStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ThreadStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn load_accessible(
        &self,
        thread_id: &ThreadId,
        actor_id: &ActorId,
        action: &'static str,
    ) -> Result<ThreadAccess> {
        require_actor(self.connection, actor_id.as_str()).await?;
        let route = require_target_route(self.connection, thread_id.as_str()).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(format!(
                "{action} requires a Thread target"
            )));
        }
        let permission_target_id = route.permission_target_id(thread_id.as_str()).to_owned();
        require_active_member(
            self.connection,
            &permission_target_id,
            actor_id.as_str(),
            action,
        )
        .await?;
        Ok(ThreadAccess {
            thread_id: thread_id.clone(),
            permission_target_id,
        })
    }

    pub(crate) async fn load_subscription(
        &self,
        thread_id: &ThreadId,
        actor_id: &ActorId,
    ) -> Result<ThreadSubscription> {
        let mut rows = self
            .connection
            .query(
                "SELECT unfollowed_at_ms FROM thread_follows
                 WHERE thread_target_id = ?1 AND actor_id = ?2",
                (thread_id.as_str(), actor_id.as_str()),
            )
            .await?;
        let state = match rows.next().await? {
            Some(row) if row.get::<Option<i64>>(0)?.is_none() => FollowState::Following,
            Some(_) | None => FollowState::NotFollowing,
        };
        Ok(ThreadSubscription::new(
            thread_id.clone(),
            actor_id.clone(),
            state,
        ))
    }

    pub(crate) async fn save_subscription(
        &self,
        subscription: &ThreadSubscription,
        outcome: FollowOutcome,
        changed_at_ms: i64,
    ) -> Result<()> {
        if !outcome.changed() {
            return Ok(());
        }
        let changed = match subscription.state() {
            FollowState::Following => {
                self.connection
                    .execute(
                        "INSERT INTO thread_follows
                         (thread_target_id, actor_id, followed_at_ms, unfollowed_at_ms)
                         VALUES (?1, ?2, ?3, NULL)
                         ON CONFLICT(thread_target_id, actor_id) DO UPDATE SET
                           followed_at_ms = excluded.followed_at_ms,
                           unfollowed_at_ms = NULL
                         WHERE thread_follows.unfollowed_at_ms IS NOT NULL",
                        (
                            subscription.thread_id().as_str(),
                            subscription.actor_id().as_str(),
                            changed_at_ms,
                        ),
                    )
                    .await?
            }
            FollowState::NotFollowing => {
                self.connection
                    .execute(
                        "UPDATE thread_follows
                         SET unfollowed_at_ms = ?3
                         WHERE thread_target_id = ?1 AND actor_id = ?2
                           AND unfollowed_at_ms IS NULL",
                        (
                            subscription.thread_id().as_str(),
                            subscription.actor_id().as_str(),
                            changed_at_ms,
                        ),
                    )
                    .await?
            }
        };
        if changed != 1 {
            return Err(CollabError::Database(format!(
                "Thread subscription transition updated {changed} rows"
            )));
        }
        Ok(())
    }

    pub(crate) async fn ensure_following(
        &self,
        thread_target_id: &str,
        actor_id: &str,
        changed_at_ms: i64,
    ) -> Result<FollowOutcome> {
        let thread_id = ThreadId::parse(thread_target_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let mut subscription = self.load_subscription(&thread_id, &actor_id).await?;
        let outcome = subscription.follow();
        self.save_subscription(&subscription, outcome, changed_at_ms)
            .await?;
        Ok(outcome)
    }

    pub(crate) async fn is_following(
        &self,
        thread_target_id: &str,
        actor_id: &str,
    ) -> Result<bool> {
        let thread_id = ThreadId::parse(thread_target_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let subscription = self.load_subscription(&thread_id, &actor_id).await?;
        Ok(subscription.state() == FollowState::Following)
    }
}
