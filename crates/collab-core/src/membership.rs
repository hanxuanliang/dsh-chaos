//! Membership mutation and role-bearing member projections.

use super::*;
use crate::db::{FromRow, QueryRows};
use crate::ids::ActorId;
use crate::profile::store::ProfileStore;

/// Presence proof that one Actor is an active member of one target. Obtained
/// exclusively through [`Membership::require`]; role-bearing checks will grow
/// fields when a caller needs them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Membership;

impl FromRow for Membership {
    fn from_row(_row: &Row) -> Result<Self> {
        Ok(Self)
    }
}

impl Membership {
    /// Certify that `actor` is an active member of `target_id`.
    pub(crate) async fn require(
        connection: &Connection,
        target_id: &str,
        actor: &Actor,
    ) -> Result<Self> {
        connection
            .query_row::<Self>(
                "SELECT 1 FROM memberships
                 WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
                (target_id, actor.id.as_str()),
            )
            .await?
            .ok_or_else(|| CollabError::PermissionDenied {
                actor_id: actor.id.clone(),
                action: "participate in",
                target_id: target_id.to_owned(),
            })
    }
}

impl CollabCore {
    /// Add or reactivate one Channel member.
    pub async fn add_member(&self, target_id: &str, actor_id: &str, added_by: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let actor = Actor::require(&transaction, &ActorId::parse(actor_id)?).await?;
        if require_target(&transaction, target_id).await? != TargetKind::Channel {
            return Err(CollabError::InvalidArgument(
                "add_member only supports Channel targets".into(),
            ));
        }
        require_owner(&transaction, target_id, added_by).await?;
        transaction
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
            transaction
                .execute(
                    "UPDATE agent_wake_state
                     SET notified_generation = 0
                     WHERE agent_id = ?1",
                    [actor_id],
                )
                .await?;
        }
        insert_target_change(
            &transaction,
            ChangeKind::MembershipChanged,
            target_id,
            actor_id,
            &[actor_id],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// List the local actor directory after authenticating the caller. A
    /// deleted Agent keeps its actors row for message authorship but loses its
    /// agents row, so the directory lists users and live Agents only.
    pub async fn list_actors(&self, actor_id: &str) -> Result<Vec<Actor>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        Actor::require(&connection, &ActorId::parse(actor_id)?).await?;
        connection
            .query_rows::<Actor>(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors
                 WHERE kind = 'user' OR id IN (SELECT actor_id FROM agents)
                 ORDER BY handle, id",
                (),
            )
            .await
    }

    /// List the active members of one Channel after authorizing the caller's
    /// own access to that Channel. This is the membership projection the web
    /// members pane must use; the actor directory is not a member list.
    pub async fn list_target_members(&self, actor_id: &str, target_id: &str) -> Result<Vec<Actor>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id).await?;
        if require_target(&connection, target_id).await? != TargetKind::Channel {
            return Err(CollabError::InvalidArgument(
                "list_target_members only supports Channel targets".into(),
            ));
        }
        connection
            .query_rows::<Actor>(
                "SELECT actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms
                 FROM memberships membership
                 JOIN actors actor ON actor.id = membership.actor_id
                 WHERE membership.target_id = ?1
                   AND membership.left_at_ms IS NULL
                 ORDER BY actor.handle, actor.id",
                (target_id,),
            )
            .await
    }

    /// List the role-bearing active roster for an exact target. Thread rosters
    /// are inherited from their parent Channel or Direct target.
    pub async fn list_target_memberships(
        &self,
        actor_id: &str,
        target_id: &str,
    ) -> Result<Vec<TargetMember>> {
        self.assert_open()?;
        CollabError::require_non_blank("actor_id", actor_id)?;
        CollabError::require_non_blank("target_id", target_id)?;
        let connection = self.connection.lock().await;
        let route = require_target_access(&connection, target_id, actor_id).await?;
        target_memberships(&connection, route.permission_target_id(target_id)).await
    }

    /// List one Agent's active top-level memberships that are also visible to
    /// the requesting actor. Thread membership is inherited and therefore is
    /// not duplicated in this projection.
    pub async fn list_agent_memberships(
        &self,
        actor_id: &str,
        agent_id: &str,
    ) -> Result<Vec<AgentMembership>> {
        self.assert_open()?;
        CollabError::require_non_blank("actor_id", actor_id)?;
        CollabError::require_non_blank("agent_id", agent_id)?;
        let connection = self.connection.lock().await;
        Actor::require(&connection, &ActorId::parse(actor_id)?).await?;
        ProfileStore::new(&connection)
            .require_profile(agent_id)
            .await?;
        agent_memberships_for(&connection, actor_id, agent_id).await
    }
}

pub(crate) fn parse_membership_role(
    target_id: &str,
    actor_id: &str,
    value: &str,
) -> Result<MembershipRole> {
    match value {
        "owner" => Ok(MembershipRole::Owner),
        "member" => Ok(MembershipRole::Member),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' member '{actor_id}' has unknown role '{other}'"
        ))),
    }
}

pub(crate) async fn target_memberships(
    connection: &Connection,
    target_id: &str,
) -> Result<Vec<TargetMember>> {
    let mut rows = connection
        .query(
            "SELECT actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms,
                    membership.role, membership.joined_at_ms
             FROM memberships membership
             JOIN actors actor ON actor.id = membership.actor_id
             WHERE membership.target_id = ?1 AND membership.left_at_ms IS NULL
             ORDER BY actor.handle, actor.id",
            [target_id],
        )
        .await?;
    let mut members = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        let role_text = row.get::<String>(5)?;
        members.push(TargetMember {
            actor: Actor {
                kind: parse_actor_kind(&id, &kind_text)?,
                id: id.clone(),
                handle: row.get(2)?,
                display_name: row.get(3)?,
                created_at_ms: row.get(4)?,
            },
            role: parse_membership_role(target_id, &id, &role_text)?,
            joined_at_ms: row.get(6)?,
        });
    }
    Ok(members)
}

pub(crate) async fn active_member_ids(
    connection: &Connection,
    target_id: &str,
) -> Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT actor_id FROM memberships
             WHERE target_id = ?1 AND left_at_ms IS NULL
             ORDER BY actor_id",
            [target_id],
        )
        .await?;
    let mut actor_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        actor_ids.push(row.get(0)?);
    }
    Ok(actor_ids)
}

pub(crate) async fn target_change_recipients(
    connection: &Connection,
    target_id: &str,
    extra_actor_ids: &[&str],
) -> Result<Vec<String>> {
    let route = TargetRoute::require(connection, target_id).await?;
    let mut recipients = BTreeSet::new();
    // Change events drive authorized UI invalidation, not attention delivery.
    // A Thread therefore addresses every active parent member even when they
    // unfollow it; only Message Delivery/wake snapshots are follower-scoped.
    let mut rows = connection
        .query(
            "SELECT actor_id
             FROM memberships
             WHERE target_id = ?1 AND left_at_ms IS NULL",
            [route.permission_target_id(target_id)],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        recipients.insert(row.get::<String>(0)?);
    }
    drop(rows);
    recipients.extend(
        extra_actor_ids
            .iter()
            .map(|actor_id| (*actor_id).to_owned()),
    );
    Ok(recipients.into_iter().collect())
}

pub(crate) async fn agent_memberships_for(
    connection: &Connection,
    actor_id: &str,
    agent_id: &str,
) -> Result<Vec<AgentMembership>> {
    let mut rows = connection
        .query(
            "SELECT target.id, target.kind, target.name, target.parent_target_id,
                    target.root_message_id, target.created_by, target.created_at_ms,
                    agent_membership.role, agent_membership.joined_at_ms
             FROM memberships agent_membership
             JOIN targets target ON target.id = agent_membership.target_id
             JOIN memberships viewer_membership
               ON viewer_membership.target_id = target.id
              AND viewer_membership.actor_id = ?1
              AND viewer_membership.left_at_ms IS NULL
             WHERE agent_membership.actor_id = ?2
               AND agent_membership.left_at_ms IS NULL
               AND target.archived_at_ms IS NULL
               AND target.kind IN ('channel', 'direct')
             ORDER BY target.kind, target.name, target.id",
            (actor_id, agent_id),
        )
        .await?;
    let mut memberships = Vec::new();
    while let Some(row) = rows.next().await? {
        let target_id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        let role_text = row.get::<String>(7)?;
        memberships.push(AgentMembership {
            target: Target {
                id: target_id.clone(),
                kind: parse_target_kind(&target_id, &kind_text)?,
                name: row.get(2)?,
                parent_target_id: row.get(3)?,
                root_message_id: row.get(4)?,
                created_by: row.get(5)?,
                created_at_ms: row.get(6)?,
            },
            role: parse_membership_role(&target_id, agent_id, &role_text)?,
            joined_at_ms: row.get(8)?,
        });
    }
    Ok(memberships)
}

pub(crate) async fn identity_context_for(
    connection: &Connection,
    agent_id: &str,
    target_id: Option<&str>,
) -> Result<IdentityContext> {
    let agent = ProfileStore::new(connection)
        .require_profile(agent_id)
        .await?;
    let Some(target_id) = target_id else {
        return Ok(IdentityContext {
            agent,
            target: None,
            membership_target: None,
            members: Vec::new(),
        });
    };
    CollabError::require_non_blank("target_id", target_id)?;
    let route = require_target_access(connection, target_id, agent_id).await?;
    let target = find_target(connection, target_id).await?;
    let membership_target_id = route.permission_target_id(target_id);
    let membership_target = if membership_target_id == target_id {
        target.clone()
    } else {
        find_target(connection, membership_target_id).await?
    };
    let members = target_memberships(connection, membership_target_id).await?;
    Ok(IdentityContext {
        agent,
        target: Some(target),
        membership_target: Some(membership_target),
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn agent_membership_directory_is_top_level_and_viewer_filtered() -> Result<()> {
        let (core, owner, alpha, _beta, channel) = fixture().await?;
        let direct = core.create_direct(&owner.id, &alpha.id).await?;
        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: owner.id.clone(),
                client_request_id: "membership-root".into(),
                text: "Open a thread".into(),
            })
            .await?
            .message;
        let _thread = core.create_thread(&root.id, &owner.id).await?;

        let memberships = core.list_agent_memberships(&owner.id, &alpha.id).await?;
        assert_eq!(memberships.len(), 2);
        assert_eq!(memberships[0].target, channel);
        assert_eq!(memberships[0].role, MembershipRole::Member);
        assert_eq!(memberships[1].target, direct);
        assert_eq!(memberships[1].role, MembershipRole::Member);
        assert!(
            memberships
                .iter()
                .all(|membership| membership.target.kind != TargetKind::Thread)
        );

        let outsider = core.create_user("outsider", "Outsider").await?;
        assert!(
            core.list_agent_memberships(&outsider.id, &alpha.id)
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[tokio::test]
    async fn identity_context_returns_role_bearing_inherited_roster() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let channel_context = core.identity_context(&alpha.id, Some(&channel.id)).await?;
        assert_eq!(channel_context.agent.actor.handle, "alpha");
        assert_eq!(channel_context.target.as_ref(), Some(&channel));
        assert_eq!(channel_context.membership_target.as_ref(), Some(&channel));
        assert_eq!(channel_context.members.len(), 3);
        assert!(
            channel_context.members.iter().any(|member| {
                member.actor.id == user.id && member.role == MembershipRole::Owner
            })
        );
        assert!(
            channel_context
                .members
                .iter()
                .any(|member| { member.actor.id == beta.id && member.actor.handle == "beta" })
        );

        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "identity-root".into(),
                text: "Discuss in a Thread".into(),
            })
            .await?
            .message;
        let thread = core.create_thread(&root.id, &user.id).await?;
        let thread_context = core.identity_context(&alpha.id, Some(&thread.id)).await?;
        assert_eq!(thread_context.target.as_ref(), Some(&thread));
        assert_eq!(thread_context.membership_target.as_ref(), Some(&channel));
        assert_eq!(thread_context.members, channel_context.members);

        let outsider = core
            .create_agent("outsider-agent", "Outsider", "/tmp/outsider-agent")
            .await?;
        assert!(matches!(
            core.identity_context(&outsider.id, Some(&channel.id)).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn list_target_members_returns_only_active_channel_members() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let outsider = core.create_user("outsider", "Outsider").await?;

        // The projection is the Channel membership, not the actor directory:
        // the fixture created extra actors that never joined this channel.
        let members = core.list_target_members(&user.id, &channel.id).await?;
        let mut member_ids: Vec<&str> = members.iter().map(|actor| actor.id.as_str()).collect();
        member_ids.sort_unstable();
        let mut expected_ids = vec![alpha.id.as_str(), beta.id.as_str(), user.id.as_str()];
        expected_ids.sort_unstable();
        assert_eq!(member_ids, expected_ids);

        // A second channel has its own membership.
        let other = core.create_channel("other", &user.id).await?;
        let other_members = core.list_target_members(&user.id, &other.id).await?;
        assert_eq!(other_members.len(), 1);
        assert_eq!(other_members[0].id, user.id);

        // Non-members are rejected, and left members disappear.
        assert!(matches!(
            core.list_target_members(&outsider.id, &channel.id).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        Ok(())
    }
}
