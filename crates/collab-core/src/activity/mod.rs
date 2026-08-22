//! Actor-scoped Activity inbox projection and Done fences.

mod model;
pub(crate) mod store;

pub use model::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind,
};

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::target::require_target_access;
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
            let route = require_target_access(connection, target_id, actor_id).await?;
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
    /// List active Activity conversations newest-first. Eligibility and Done
    /// are actor-specific; count and page are read from one database snapshot.
    pub async fn inbox_list(
        &self,
        actor_id: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ActivityInboxPage> {
        if limit == 0 || limit > 50 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 50".into(),
            ));
        }
        let cursor = cursor.map(ActivityCursor::parse).transpose()?;
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let store = ActivityStore::new(connection);
            let active_count = store.active_count(actor_id).await?;
            let mut items = store.inbox_page(actor_id, cursor.as_ref(), limit).await?;

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
}
