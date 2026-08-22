//! Bootstrap snapshot and change replay projections.

use crate::actor::ActorId;
use crate::target::targets_for_actor;
use crate::task::store::TaskStore;
use crate::thread::query::followed_thread_ids_for_actor;
use crate::{Actor, ChangeEvent, CollabCore, CollabError, CollabSnapshot, Result};

use super::store::{change_retention_floor, changes_after, latest_change_seq};

impl CollabCore {
    /// Return one authorization-filtered bootstrap projection and the global
    /// durable change cursor observed in the same connection critical section.
    pub async fn snapshot(&self, actor_id: &str) -> Result<CollabSnapshot> {
        self.read(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let cursor = latest_change_seq(connection).await?;
            let targets = targets_for_actor(connection, actor_id).await?;
            let followed_thread_ids = followed_thread_ids_for_actor(connection, actor_id).await?;
            let tasks = TaskStore::new(connection)
                .tasks_for_actor(actor_id, None)
                .await?;
            Ok(CollabSnapshot {
                actor,
                cursor,
                targets,
                followed_thread_ids,
                tasks,
            })
        })
        .await
    }

    /// Return durable changes addressed to one actor after a global cursor.
    pub async fn list_changes(
        &self,
        actor_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ChangeEvent>> {
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 500 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 500".into(),
            ));
        }
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let minimum_cursor = change_retention_floor(connection).await?;
            let maximum_cursor = latest_change_seq(connection).await?;
            if after_seq < minimum_cursor || after_seq > maximum_cursor {
                return Err(CollabError::ChangeCursorOutOfRange {
                    after_seq,
                    minimum_cursor,
                    maximum_cursor,
                });
            }
            changes_after(connection, actor_id, after_seq, limit).await
        })
        .await
    }
}
