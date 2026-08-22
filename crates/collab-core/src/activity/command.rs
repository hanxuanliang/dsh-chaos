use crate::changefeed::insert_change;
use crate::ids::{ActorId, ThreadId};
use crate::target::require_target_access;
use crate::thread::store::ThreadStore;
use crate::{
    Actor, ChangeKind, CollabCore, CollabError, Result, TargetKind, not_found, now_ms,
    require_non_empty,
};

use super::store::ActivityStore;

impl CollabCore {
    /// Record an actor's Activity disposition fence. The fence only advances;
    /// a newer Message sequence automatically makes the conversation active.
    pub async fn inbox_done(
        &self,
        actor_id: &str,
        target_id: &str,
        through_seq: i64,
    ) -> Result<()> {
        require_non_empty("actor_id", actor_id)?;
        require_non_empty("target_id", target_id)?;
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
                return Err(not_found("active Thread follow", target_id));
            }

            let store = ActivityStore::new(connection);
            let latest_seq = store
                .latest_seq(target_id)
                .await?
                .ok_or_else(|| not_found("target activity", target_id))?;
            if through_seq > latest_seq {
                return Err(CollabError::InvalidArgument(format!(
                    "through_seq {through_seq} is newer than target activity {latest_seq}"
                )));
            }

            if store
                .set_done_fence(actor_id, target_id, through_seq, now)
                .await?
            {
                insert_change(
                    connection,
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
