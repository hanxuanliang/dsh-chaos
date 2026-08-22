//! Membership mutation.

use crate::actor::{Actor, ActorId};
use crate::changefeed::insert_target_change;
use crate::target::require_target;
use crate::{ActorKind, ChangeKind, CollabCore, CollabError, Result, TargetKind, now_ms};

use super::model::Membership;

impl CollabCore {
    /// Add or reactivate one Channel member.
    pub async fn add_member(&self, target_id: &str, actor_id: &str, added_by: &str) -> Result<()> {
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if require_target(connection, target_id).await? != TargetKind::Channel {
                return Err(CollabError::InvalidArgument(
                    "add_member only supports Channel targets".into(),
                ));
            }
            Membership::require_owner(connection, target_id, added_by).await?;
            connection
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, 'member', ?3, NULL)
                     ON CONFLICT(target_id, actor_id) DO UPDATE SET
                       role = CASE
                         WHEN memberships.role = 'owner' THEN 'owner'
                         ELSE 'member'
                       END,
                       joined_at_ms = excluded.joined_at_ms,
                       left_at_ms = NULL",
                    (target_id, actor_id, now),
                )
                .await?;
            if actor.kind == ActorKind::Agent {
                connection
                    .execute(
                        "UPDATE agent_wake_state
                         SET notified_generation = 0
                         WHERE agent_id = ?1",
                        [actor_id],
                    )
                    .await?;
            }
            insert_target_change(
                connection,
                ChangeKind::MembershipChanged,
                target_id,
                actor_id,
                &[actor_id],
                now,
            )
            .await?;
            Ok(())
        })
        .await
    }
}
