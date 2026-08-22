//! Channel and Direct target creation.

use crate::actor::{Actor, ActorId};
use crate::changefeed::insert_target_change;
use crate::{ChangeKind, CollabCore, CollabError, Result, Target, TargetKind, new_id, now_ms};

use super::store::find_target;

impl CollabCore {
    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(&self, name: &str, creator_id: &str) -> Result<Target> {
        require_non_blank!("channel name" = name);
        require_non_blank!(creator_id);
        let creator_id = ActorId::parse(creator_id)?;
        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.to_owned(),
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.as_str().to_owned(),
            created_at_ms: now,
        };
        self.write(async |connection| {
            Actor::require(connection, &creator_id).await?;
            connection
                .execute(
                    "INSERT INTO targets
                     (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL)",
                    (
                        target.id.as_str(),
                        target.kind.as_str(),
                        target.name.as_str(),
                        target.created_by.as_str(),
                        target.created_at_ms,
                    ),
                )
                .await?;
            connection
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, 'owner', ?3, NULL)",
                    (target.id.as_str(), target.created_by.as_str(), now),
                )
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TargetCreated,
                &target.id,
                &target.id,
                &[],
                now,
            )
            .await?;
            Ok(target)
        })
        .await
    }

    /// Return the one stable Direct target for an unordered pair of actors,
    /// creating it and its two memberships when absent.
    pub async fn create_direct(&self, actor_id: &str, peer_id: &str) -> Result<Target> {
        require_non_blank!(actor_id, peer_id);
        if actor_id == peer_id {
            return Err(CollabError::InvalidArgument(
                "a Direct target requires two distinct actors".into(),
            ));
        }
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            let peer = Actor::require(connection, &ActorId::parse(peer_id)?).await?;
            let (low, high) = if actor.id < peer.id {
                (&actor, &peer)
            } else {
                (&peer, &actor)
            };

            let mut rows = connection
                .query(
                    "SELECT target_id FROM direct_pairs
                     WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                    (low.id.as_str(), high.id.as_str()),
                )
                .await?;
            if let Some(row) = rows.next().await? {
                let target_id = row.get::<String>(0)?;
                drop(rows);
                return find_target(connection, &target_id).await;
            }
            drop(rows);

            let target = Target {
                id: new_id(),
                kind: TargetKind::Direct,
                name: format!("@{} ↔ @{}", low.handle, high.handle),
                parent_target_id: None,
                root_message_id: None,
                created_by: actor.id.clone(),
                created_at_ms: now,
            };
            connection
                .execute(
                    "INSERT INTO targets
                     (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL)",
                    (
                        target.id.as_str(),
                        target.kind.as_str(),
                        target.name.as_str(),
                        target.created_by.as_str(),
                        now,
                    ),
                )
                .await?;
            for (member_id, role) in [(&actor.id, "owner"), (&peer.id, "member")] {
                connection
                    .execute(
                        "INSERT INTO memberships
                         (target_id, actor_id, role, joined_at_ms, left_at_ms)
                         VALUES (?1, ?2, ?3, ?4, NULL)",
                        (target.id.as_str(), member_id.as_str(), role, now),
                    )
                    .await?;
            }
            connection
                .execute(
                    "INSERT INTO direct_pairs
                     (target_id, actor_low_id, actor_high_id, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    (
                        target.id.as_str(),
                        low.id.as_str(),
                        high.id.as_str(),
                        now,
                    ),
                )
                .await?;
            insert_target_change(
                connection,
                ChangeKind::TargetCreated,
                &target.id,
                &target.id,
                &[],
                now,
            )
            .await?;
            Ok(target)
        })
        .await
    }
}
