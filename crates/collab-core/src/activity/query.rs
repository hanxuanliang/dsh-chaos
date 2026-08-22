use crate::actor::ActorId;
use crate::{ActivityInboxPage, Actor, CollabCore, CollabError, Result};

use super::model::ActivityCursor;
use super::store::ActivityStore;

impl CollabCore {
    /// List active Activity conversations newest-first. Eligibility and Done
    /// are actor-specific; count and page are read from one database snapshot.
    pub async fn inbox_list(
        &self,
        actor_id: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ActivityInboxPage> {
        require_non_blank!(actor_id);
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
