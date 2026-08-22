use crate::actor::ActorId;
use crate::{CollabCore, Result};

use super::model::{RootMessageIds, ThreadSummary};
use super::store::ThreadStore;

impl CollabCore {
    /// Return reply counts and the three most recent distinct repliers for up
    /// to 100 root Messages. Inaccessible and empty Threads are omitted.
    pub async fn thread_summaries(
        &self,
        actor_id: &str,
        root_message_ids: &[String],
    ) -> Result<Vec<ThreadSummary>> {
        let actor_id = ActorId::parse(actor_id)?;
        let roots = RootMessageIds::parse(root_message_ids, 100)?;
        self.read(async |connection| {
            ThreadStore::new(connection)
                .summaries(&actor_id, &roots)
                .await
        })
        .await
    }
}

pub(crate) async fn followed_thread_ids_for_actor(
    connection: &turso::Connection,
    actor_id: &str,
) -> Result<Vec<String>> {
    let actor_id = ActorId::parse(actor_id)?;
    ThreadStore::new(connection)
        .followed_thread_ids(&actor_id)
        .await
}
