//! Actor-scoped Activity inbox projection and Done fences.

mod model;
pub(crate) mod store;

pub use model::{
    ActivityFilter, ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask,
    ActivityTitleKind,
};

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::target::AccessGrant;
use crate::thread::ThreadId;
use crate::thread::store::ThreadStore;
use crate::{Actor, ChangeKind, CollabCore, CollabError, Result, TargetKind, now_ms};

use model::ActivityCursor;
use store::ActivityStore;

impl CollabCore {
    /// Record an actor's Activity disposition fence. The fence only advances;
    /// a newer Message sequence automatically makes the conversation active.
    pub async fn inbox_done(
        &self,
        actor_id: &str,
        target_id: &str,
        through_seq: i64,
    ) -> Result<()> {
        if through_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "through_seq must be a positive integer".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let route = AccessGrant::require(connection, target_id, actor_id)
                .await?
                .route;
            if route.kind == TargetKind::Thread
                && !ThreadStore::new(connection)
                    .is_following(&ThreadId::parse(target_id)?, &ActorId::parse(actor_id)?)
                    .await?
            {
                return Err(CollabError::NotFound {
                    entity: "active Thread follow",
                    id: target_id.to_owned(),
                });
            }

            let store = ActivityStore::new(connection);
            let latest_seq =
                store
                    .latest_seq(target_id)
                    .await?
                    .ok_or_else(|| CollabError::NotFound {
                        entity: "target activity",
                        id: target_id.to_owned(),
                    })?;
            if through_seq > latest_seq {
                return Err(CollabError::InvalidArgument(format!(
                    "through_seq {through_seq} is newer than target activity {latest_seq}"
                )));
            }

            if store
                .set_done_fence(actor_id, target_id, through_seq, now)
                .await?
            {
                ChangeStore::new(connection)
                    .insert_change(
                        ChangeKind::ActivityDoneChanged,
                        Some(target_id),
                        target_id,
                        &[actor_id.to_owned()],
                        now,
                    )
                    .await?;
            }
            Ok(())
        })
        .await
    }
}

impl CollabCore {
    /// List Activity conversations newest-first under one filter. Unread
    /// returns only conversations with activity past the actor's Done fence;
    /// All returns every conversation with its done flag. `active_count` is
    /// always the unread count, so it can badge the Unread pill.
    pub async fn inbox_list(
        &self,
        actor_id: &str,
        limit: u32,
        cursor: Option<&str>,
        filter: Option<&str>,
    ) -> Result<ActivityInboxPage> {
        if limit == 0 || limit > 50 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 50".into(),
            ));
        }
        let filter = ActivityFilter::parse(filter)?;
        let unread = filter == ActivityFilter::Unread;
        let cursor = cursor.map(ActivityCursor::parse).transpose()?;
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = ActivityStore::new(connection);
            let active_count = store.active_count(actor_id).await?;
            let mut items = store
                .inbox_page(actor_id, cursor.as_ref(), limit, unread)
                .await?;

            let has_more = items.len() > limit as usize;
            if has_more {
                items.truncate(limit as usize);
            }
            let next_cursor = has_more
                .then(|| items.last().map(ActivityCursor::for_item))
                .flatten();
            Ok(ActivityInboxPage {
                items,
                next_cursor,
                active_count,
            })
        })
        .await
    }

    /// Advance the actor's Done fence to the latest activity of every active
    /// unread conversation, returning how many fences moved. One change
    /// notification covers the whole batch; the fence's forward-only rule
    /// keeps a repeat call a no-op returning zero.
    pub async fn inbox_done_all(&self, actor_id: &str) -> Result<u32> {
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = ActivityStore::new(connection);
            let conversations = store.active_conversations(actor_id).await?;
            let mut advanced = 0u32;
            for (target_id, last_seq) in &conversations {
                if store
                    .set_done_fence(actor_id, target_id, *last_seq, now)
                    .await?
                {
                    advanced += 1;
                }
            }
            if advanced > 0 {
                ChangeStore::new(connection)
                    .insert_change(
                        ChangeKind::ActivityDoneChanged,
                        None,
                        actor_id,
                        &[actor_id.to_owned()],
                        now,
                    )
                    .await?;
            }
            Ok(advanced)
        })
        .await
    }
}
