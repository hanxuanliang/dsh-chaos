use std::collections::HashMap;

use turso::{Connection, Row};

use crate::actor::ActorId;
use crate::db::{FromRow, QueryRows, placeholders};
use crate::target::TargetStore;
use crate::{Actor, CollabError, Result, Target};

use super::model::{
    FollowOutcome, FollowState, RootMessageIds, ThreadId, ThreadSubscription, ThreadSummary,
};

struct TargetIdRow(String);

impl FromRow for TargetIdRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct ThreadCountRow {
    thread_id: String,
    root_message_id: String,
    reply_count: i64,
    last_reply_at_ms: Option<i64>,
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
    fn into_summary(self) -> ThreadSummary {
        ThreadSummary {
            root_message_id: self.root_message_id,
            thread_id: self.thread_id,
            reply_count: self.reply_count,
            last_reply_at_ms: self.last_reply_at_ms,
            recent_replier_ids: Vec::new(),
        }
    }
}

struct ThreadReplierRow {
    thread_id: String,
    author_id: String,
}

impl FromRow for ThreadReplierRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            thread_id: row.get(0)?,
            author_id: row.get(1)?,
        })
    }
}

struct FollowedThreadId(String);

impl FromRow for FollowedThreadId {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct SubscriptionRow {
    unfollowed_at_ms: Option<i64>,
}

impl FromRow for SubscriptionRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            unfollowed_at_ms: row.get(0)?,
        })
    }
}

impl From<SubscriptionRow> for FollowState {
    fn from(row: SubscriptionRow) -> Self {
        if row.unfollowed_at_ms.is_none() {
            Self::Following
        } else {
            Self::NotFollowing
        }
    }
}

pub(crate) struct ThreadStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ThreadStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn load_subscription(
        &self,
        thread_id: &ThreadId,
        actor_id: &ActorId,
    ) -> Result<ThreadSubscription> {
        let state = self
            .connection
            .query_row::<SubscriptionRow>(
                "SELECT unfollowed_at_ms FROM thread_follows
                 WHERE thread_target_id = ?1 AND actor_id = ?2",
                (thread_id.as_str(), actor_id.as_str()),
            )
            .await?
            .map_or(FollowState::NotFollowing, FollowState::from);
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

    pub(crate) async fn end_follows_for_parent(
        &self,
        parent_target_id: &str,
        now: i64,
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE thread_follows
                 SET unfollowed_at_ms = ?2
                 WHERE unfollowed_at_ms IS NULL
                   AND thread_target_id IN (
                     SELECT id FROM targets WHERE parent_target_id = ?1 AND kind = 'thread'
                   )",
                (parent_target_id, now),
            )
            .await?;
        Ok(())
    }

    /// Return the active Thread rooted at one Message, when it exists.
    pub(crate) async fn find_by_root(&self, root_message_id: &str) -> Result<Option<Target>> {
        let Some(TargetIdRow(target_id)) = self
            .connection
            .query_row::<TargetIdRow>(
                "SELECT id FROM targets WHERE root_message_id = ?1",
                [root_message_id],
            )
            .await?
        else {
            return Ok(None);
        };
        TargetStore::new(self.connection)
            .find(&target_id)
            .await
            .map(Some)
    }

    pub(crate) async fn ensure_following(
        &self,
        thread_id: &ThreadId,
        actor_id: &ActorId,
        changed_at_ms: i64,
    ) -> Result<FollowOutcome> {
        let mut subscription = self.load_subscription(thread_id, actor_id).await?;
        let outcome = subscription.follow();
        self.save_subscription(&subscription, outcome, changed_at_ms)
            .await?;
        Ok(outcome)
    }

    pub(crate) async fn is_following(
        &self,
        thread_id: &ThreadId,
        actor_id: &ActorId,
    ) -> Result<bool> {
        let subscription = self.load_subscription(thread_id, actor_id).await?;
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
        Actor::require(self.connection, actor_id).await?;
        let root_placeholders = placeholders(roots.as_slice().len());
        let actor_parameter = roots.as_slice().len() + 1;
        let counts_sql = format!(
            "SELECT thread.id, thread.root_message_id, COUNT(message.id),
                    MAX(message.created_at_ms)
             FROM v_target_access access
             JOIN targets thread
               ON thread.id = access.target_id AND thread.kind = 'thread'
             LEFT JOIN messages message ON message.target_id = thread.id
             WHERE access.actor_id = ?{actor_parameter}
               AND thread.root_message_id IN ({root_placeholders})
               AND EXISTS (
                 SELECT 1 FROM targets parent
                 WHERE parent.id = thread.parent_target_id
                   AND parent.kind IN ('channel', 'direct')
                   AND parent.archived_at_ms IS NULL
               )
             GROUP BY thread.id, thread.root_message_id
             ORDER BY thread.root_message_id, thread.id"
        );
        let count_parameters: Vec<&str> = roots
            .as_slice()
            .iter()
            .map(String::as_str)
            .chain([actor_id.as_str()])
            .collect();
        let summaries: Vec<ThreadSummary> = self
            .connection
            .query_rows(&counts_sql, count_parameters)
            .await?
            .into_iter()
            .filter(|count: &ThreadCountRow| count.reply_count > 0)
            .map(ThreadCountRow::into_summary)
            .collect();
        if summaries.is_empty() {
            return Ok(summaries);
        }
        let visible_thread_ids: Vec<&str> = summaries
            .iter()
            .map(|summary| summary.thread_id.as_str())
            .collect();
        let replier_sql = format!(
            "SELECT target_id, author_id, MAX(seq) AS latest_seq
             FROM messages
             WHERE target_id IN ({})
             GROUP BY target_id, author_id
             ORDER BY target_id, latest_seq DESC, author_id",
            placeholders(visible_thread_ids.len()),
        );
        let mut recent_by_thread: HashMap<String, Vec<String>> = HashMap::new();
        for replier in self
            .connection
            .query_rows::<ThreadReplierRow>(&replier_sql, visible_thread_ids)
            .await?
        {
            let recent = recent_by_thread.entry(replier.thread_id).or_default();
            if recent.len() < 3 {
                recent.push(replier.author_id);
            }
        }
        Ok(summaries
            .into_iter()
            .map(|mut summary| {
                if let Some(recent) = recent_by_thread.remove(&summary.thread_id) {
                    summary.recent_replier_ids = recent;
                }
                summary
            })
            .collect())
    }

    pub(crate) async fn followed_thread_ids(&self, actor_id: &ActorId) -> Result<Vec<String>> {
        let rows: Vec<FollowedThreadId> = self
            .connection
            .query_rows(
                "SELECT follow.thread_target_id
             FROM thread_follows follow
             JOIN v_target_access access
               ON access.target_id = follow.thread_target_id
              AND access.actor_id = follow.actor_id
             JOIN targets thread ON thread.id = follow.thread_target_id
             WHERE follow.actor_id = ?1
               AND follow.unfollowed_at_ms IS NULL
               AND thread.kind = 'thread'
             ORDER BY thread.created_at_ms DESC, follow.thread_target_id",
                [actor_id.as_str()],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }
}
