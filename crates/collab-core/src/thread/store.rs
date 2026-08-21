use std::collections::HashMap;

use turso::params_from_iter;
use turso::Connection;

use crate::actor::require_actor;
use crate::db::{placeholders, query_all};
use crate::ids::{ActorId, ThreadId};
use crate::target::{require_active_member, require_target_route};
use crate::{CollabError, Result, TargetKind};

use super::model::{
    FollowOutcome, FollowState, FollowedThreadId, RootMessageIds, ThreadAccess, ThreadCountRow,
    ThreadReplierRow, ThreadSubscription, ThreadSummary,
};

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

    pub(crate) async fn summaries(
        &self,
        actor_id: &ActorId,
        roots: &RootMessageIds,
    ) -> Result<Vec<ThreadSummary>> {
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        require_actor(self.connection, actor_id.as_str()).await?;
        let root_placeholders = placeholders(roots.as_slice().len());
        let actor_parameter = roots.as_slice().len() + 1;
        let counts_sql = format!(
            "SELECT thread.id, thread.root_message_id, COUNT(message.id),
                    MAX(message.created_at_ms)
             FROM targets thread
             JOIN targets parent
               ON parent.id = thread.parent_target_id
              AND parent.kind IN ('channel', 'direct')
              AND parent.archived_at_ms IS NULL
             JOIN memberships membership
               ON membership.target_id = parent.id
              AND membership.actor_id = ?{actor_parameter}
              AND membership.left_at_ms IS NULL
             LEFT JOIN messages message ON message.target_id = thread.id
             WHERE thread.kind = 'thread'
               AND thread.archived_at_ms IS NULL
               AND thread.root_message_id IN ({root_placeholders})
             GROUP BY thread.id, thread.root_message_id
             ORDER BY thread.root_message_id, thread.id"
        );
        let mut count_parameters = roots.as_slice().to_vec();
        count_parameters.push(actor_id.as_str().to_owned());
        let counts: Vec<ThreadCountRow> =
            query_all(self.connection, &counts_sql, params_from_iter(count_parameters)).await?;
        let mut summaries: Vec<ThreadSummary> = Vec::new();
        let mut index_by_thread = HashMap::new();
        for count in counts {
            if count.reply_count == 0 {
                continue;
            }
            let thread_id = count.thread_id.clone();
            index_by_thread.insert(thread_id, summaries.len());
            summaries.push(count.into_summary());
        }
        if summaries.is_empty() {
            return Ok(summaries);
        }
        let visible_thread_ids: Vec<String> = summaries
            .iter()
            .map(|summary| summary.thread_id.clone())
            .collect();
        let replier_sql = format!(
            "SELECT target_id, author_id, MAX(seq) AS latest_seq
             FROM messages
             WHERE target_id IN ({})
             GROUP BY target_id, author_id
             ORDER BY target_id, latest_seq DESC, author_id",
            placeholders(visible_thread_ids.len()),
        );
        let repliers: Vec<ThreadReplierRow> = query_all(
            self.connection,
            &replier_sql,
            params_from_iter(visible_thread_ids),
        )
        .await?;
        for replier in repliers {
            if let Some(&index) = index_by_thread.get(&replier.thread_id) {
                let recent = &mut summaries[index].recent_replier_ids;
                if recent.len() < 3 {
                    recent.push(replier.author_id);
                }
            }
        }
        Ok(summaries)
    }

    pub(crate) async fn followed_thread_ids(&self, actor_id: &ActorId) -> Result<Vec<String>> {
        let rows: Vec<FollowedThreadId> = query_all(
            self.connection,
            "SELECT follow.thread_target_id
             FROM thread_follows follow
             JOIN targets thread
               ON thread.id = follow.thread_target_id AND thread.kind = 'thread'
             JOIN targets parent
               ON parent.id = thread.parent_target_id
              AND parent.kind IN ('channel', 'direct')
             JOIN memberships membership
               ON membership.target_id = thread.parent_target_id
              AND membership.actor_id = follow.actor_id
             WHERE follow.actor_id = ?1
               AND follow.unfollowed_at_ms IS NULL
               AND thread.archived_at_ms IS NULL
               AND parent.archived_at_ms IS NULL
               AND membership.left_at_ms IS NULL
             ORDER BY thread.created_at_ms DESC, follow.thread_target_id",
            [actor_id.as_str()],
        )
        .await?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }
}
