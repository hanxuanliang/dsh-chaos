use crate::ids::ActorId;
use crate::target::require_target_access;
use crate::{Actor, CollabCore, Result, Task};

use super::store::TaskStore;

impl CollabCore {
    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if let Some(target_id) = target_id {
                require_target_access(connection, target_id, actor_id).await?;
            }
            TaskStore::new(connection)
                .tasks_for_actor(actor_id, target_id)
                .await
        })
        .await
    }
}
