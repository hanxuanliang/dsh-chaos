//! Channel and Direct operations plus shared target authorization routes.

use super::*;
use crate::actor::ActorId;

impl CollabCore {
    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(&self, name: &str, creator_id: &str) -> Result<Target> {
        self.assert_open()?;
        CollabError::require_non_blank("channel name", name)?;
        CollabError::require_non_blank("creator_id", creator_id)?;

        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.to_owned(),
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.to_owned(),
            created_at_ms: now,
        };

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        Actor::require(&transaction, &ActorId::parse(creator_id)?).await?;
        transaction
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
        transaction
            .execute(
                "INSERT INTO memberships
                 (target_id, actor_id, role, joined_at_ms, left_at_ms)
                 VALUES (?1, ?2, 'owner', ?3, NULL)",
                (target.id.as_str(), target.created_by.as_str(), now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Return the one stable Direct target for an unordered pair of actors,
    /// creating it and its two memberships when absent.
    pub async fn create_direct(&self, actor_id: &str, peer_id: &str) -> Result<Target> {
        self.assert_open()?;
        CollabError::require_non_blank("actor_id", actor_id)?;
        CollabError::require_non_blank("peer_id", peer_id)?;
        if actor_id == peer_id {
            return Err(CollabError::InvalidArgument(
                "a Direct target requires two distinct actors".into(),
            ));
        }
        let (actor_low_id, actor_high_id) = if actor_id < peer_id {
            (actor_id, peer_id)
        } else {
            (peer_id, actor_id)
        };
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        Actor::require(&transaction, &ActorId::parse(actor_id)?).await?;
        Actor::require(&transaction, &ActorId::parse(peer_id)?).await?;

        let mut rows = transaction
            .query(
                "SELECT target_id FROM direct_pairs
                 WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                (actor_low_id, actor_high_id),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let target_id = row.get::<String>(0)?;
            drop(rows);
            let target = find_target(&transaction, &target_id).await?;
            transaction.commit().await?;
            return Ok(target);
        }
        drop(rows);

        let low_handle = actor_handle(&transaction, actor_low_id).await?;
        let high_handle = actor_handle(&transaction, actor_high_id).await?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Direct,
            name: format!("@{low_handle} ↔ @{high_handle}"),
            parent_target_id: None,
            root_message_id: None,
            created_by: actor_id.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, 'direct', ?2, NULL, NULL, ?3, ?4, NULL)",
                (
                    target.id.as_str(),
                    target.name.as_str(),
                    target.created_by.as_str(),
                    now,
                ),
            )
            .await?;
        for (member_id, role) in [(actor_id, "owner"), (peer_id, "member")] {
            transaction
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, ?3, ?4, NULL)",
                    (target.id.as_str(), member_id, role, now),
                )
                .await?;
        }
        transaction
            .execute(
                "INSERT INTO direct_pairs
                 (target_id, actor_low_id, actor_high_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                (target.id.as_str(), actor_low_id, actor_high_id, now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TargetRoute {
    pub(crate) kind: TargetKind,
    pub(crate) parent_target_id: Option<String>,
}

impl TargetRoute {
    /// Load one active target's authorization route, failing when absent.
    /// Existence and topology proof: a Thread must carry a non-Thread parent.
    pub(crate) async fn require(connection: &Connection, target_id: &str) -> Result<Self> {
        let mut rows = connection
            .query(
                "SELECT kind, parent_target_id
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            });
        };
        let kind_text = row.get::<String>(0)?;
        let parent_target_id = row.get::<Option<String>>(1)?;
        drop(rows);
        let kind = parse_target_kind(target_id, &kind_text)?;
        if kind == TargetKind::Thread && parent_target_id.is_none() {
            return Err(CollabError::Database(format!(
                "Thread target '{target_id}' has no parent target"
            )));
        }
        if kind != TargetKind::Thread && parent_target_id.is_some() {
            return Err(CollabError::Database(format!(
                "non-Thread target '{target_id}' unexpectedly has a parent"
            )));
        }
        if let Some(parent_target_id) = parent_target_id.as_deref() {
            let mut parent_rows = connection
                .query(
                    "SELECT kind FROM targets
                     WHERE id = ?1 AND archived_at_ms IS NULL",
                    [parent_target_id],
                )
                .await?;
            let Some(parent_row) = parent_rows.next().await? else {
                return Err(CollabError::NotFound {
                    entity: "active Thread parent target",
                    id: parent_target_id.to_owned(),
                });
            };
            let parent_kind_text = parent_row.get::<String>(0)?;
            let parent_kind = parse_target_kind(parent_target_id, &parent_kind_text)?;
            if parent_kind == TargetKind::Thread {
                return Err(CollabError::Database(format!(
                    "Thread target '{target_id}' has a Thread parent"
                )));
            }
        }
        Ok(Self {
            kind,
            parent_target_id,
        })
    }

    pub(crate) fn permission_target_id<'a>(&'a self, exact_target_id: &'a str) -> &'a str {
        self.parent_target_id.as_deref().unwrap_or(exact_target_id)
    }
}

pub(crate) fn parse_target_kind(target_id: &str, value: &str) -> Result<TargetKind> {
    match value {
        "channel" => Ok(TargetKind::Channel),
        "direct" => Ok(TargetKind::Direct),
        "thread" => Ok(TargetKind::Thread),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' has unknown kind '{other}'"
        ))),
    }
}

pub(crate) async fn require_target(connection: &Connection, target_id: &str) -> Result<TargetKind> {
    Ok(TargetRoute::require(connection, target_id).await?.kind)
}

/// Certify that `actor_id` exists and is an active member of `target_id`'s
/// permission target, returning the resolved route.
pub(crate) async fn require_target_access(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<TargetRoute> {
    let route = TargetRoute::require(connection, target_id).await?;
    let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
    Membership::require(connection, route.permission_target_id(target_id), &actor).await?;
    Ok(route)
}

pub(crate) async fn require_owner(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<()> {
    if is_owner(connection, target_id, actor_id).await? {
        return Ok(());
    }
    Err(CollabError::PermissionDenied {
        actor_id: actor_id.to_owned(),
        action: "manage",
        target_id: target_id.to_owned(),
    })
}

pub(crate) async fn is_owner(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2
               AND role = 'owner' AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

pub(crate) async fn is_active_member(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

pub(crate) async fn actor_handle(connection: &Connection, actor_id: &str) -> Result<String> {
    let mut rows = connection
        .query("SELECT handle FROM actors WHERE id = ?1", [actor_id])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Err(CollabError::NotFound {
            entity: "actor",
            id: actor_id.to_owned(),
        }),
    }
}

pub(crate) async fn find_target(connection: &Connection, target_id: &str) -> Result<Target> {
    let mut rows = connection
        .query(
            "SELECT id, kind, name, parent_target_id, root_message_id,
                    created_by, created_at_ms
             FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
            [target_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::NotFound {
            entity: "active target",
            id: target_id.to_owned(),
        });
    };
    let kind_text = row.get::<String>(1)?;
    Ok(Target {
        id: row.get(0)?,
        kind: parse_target_kind(target_id, &kind_text)?,
        name: row.get(2)?,
        parent_target_id: row.get(3)?,
        root_message_id: row.get(4)?,
        created_by: row.get(5)?,
        created_at_ms: row.get(6)?,
    })
}

pub(crate) async fn targets_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<Target>> {
    let mut rows = connection
        .query(
            "SELECT DISTINCT target.id, target.kind, target.name,
                    target.parent_target_id, target.root_message_id,
                    target.created_by, target.created_at_ms
             FROM targets target
             JOIN memberships membership
               ON membership.target_id = CASE
                 WHEN target.kind = 'thread' THEN target.parent_target_id
                 ELSE target.id
               END
              AND membership.actor_id = ?1
              AND membership.left_at_ms IS NULL
             WHERE target.archived_at_ms IS NULL
               AND (
                 target.kind <> 'thread'
                 OR EXISTS (
                   SELECT 1 FROM targets parent
                   WHERE parent.id = target.parent_target_id
                     AND parent.kind IN ('channel', 'direct')
                     AND parent.archived_at_ms IS NULL
                 )
               )
             ORDER BY target.created_at_ms, target.id",
            [actor_id],
        )
        .await?;
    let mut targets = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        targets.push(Target {
            kind: parse_target_kind(&id, &kind_text)?,
            id,
            name: row.get(2)?,
            parent_target_id: row.get(3)?,
            root_message_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at_ms: row.get(6)?,
        });
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn direct_target_is_unique_for_an_unordered_actor_pair() -> Result<()> {
        let (core, user, alpha, beta, _channel) = fixture().await?;
        let direct = core.create_direct(&alpha.id, &beta.id).await?;
        let reversed = core.create_direct(&beta.id, &alpha.id).await?;
        assert_eq!(direct, reversed);
        assert_eq!(direct.kind, TargetKind::Direct);
        assert!(direct.parent_target_id.is_none());
        assert!(direct.root_message_id.is_none());

        let sent = core
            .send_message(SendMessageRequest {
                target_id: direct.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "direct-message".into(),
                text: "only beta receives this".into(),
            })
            .await?;
        assert_eq!(sent.recipient_ids, vec![beta.id.clone()]);
        assert_eq!(
            core.read_messages(&beta.id, &direct.id, 0, 10).await?,
            vec![sent.message]
        );
        assert!(matches!(
            core.read_messages(&user.id, &direct.id, 0, 10).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.add_member(&direct.id, &user.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }
}
