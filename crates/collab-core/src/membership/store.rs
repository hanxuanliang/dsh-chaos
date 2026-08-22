//! Membership projections and inbox identity context shared by other verticals.

use std::collections::BTreeSet;

use turso::Connection;

use crate::actor::parse_actor_kind;
use crate::profile::store::ProfileStore;
use crate::target::{TargetRoute, find_target, parse_target_kind, require_target_access};
use crate::{Actor, AgentMembership, CollabError, IdentityContext, Result, Target, TargetMember};

use super::model::parse_membership_role;

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
