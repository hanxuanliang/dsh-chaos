//! Membership mutation, role evidence, and member projections.

use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use std::collections::BTreeSet;

use crate::actor::ActorId;
use crate::actor::{Actor, parse_actor_kind};
use crate::changefeed::insert_target_change;
use crate::db::{FromRow, QueryRows};
use crate::profile::store::ProfileStore;
use crate::target::{
    TargetRoute, find_target, parse_target_kind, require_target, require_target_access,
};
use crate::{
    AgentProfile, ChangeKind, CollabCore, CollabError, Result, Target, TargetKind, now_ms,
};

// ── 类型 ─────────────────────────────────────────────────────────────────────

/// Role of one active target member.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    Owner,
    Member,
}

impl MembershipRole {
    #[cfg(feature = "napi")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }
}

/// One role-bearing target member, enriched with its stable human-readable identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TargetMember {
    pub actor: Actor,
    pub role: MembershipRole,
    pub joined_at_ms: i64,
}

/// One active top-level target membership visible to the requesting actor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentMembership {
    pub target: Target,
    pub role: MembershipRole,
    pub joined_at_ms: i64,
}

/// Authoritative model-facing identity plus optional exact target context.
///
/// A Thread inherits its member roster from `membership_target`, which is its
/// parent Channel or Direct target. For non-Thread targets both targets match.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdentityContext {
    pub agent: AgentProfile,
    pub target: Option<Target>,
    pub membership_target: Option<Target>,
    pub members: Vec<TargetMember>,
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

// ── 证据 ─────────────────────────────────────────────────────────────────────

struct RoleRow(String);

impl FromRow for RoleRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

/// Role-bearing presence proof that one Actor is an active member of one
/// target. Obtained exclusively through [`Membership::require`] and
/// [`Membership::require_owner`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Membership {
    role: MembershipRole,
}

impl Membership {
    /// Certify that `actor` is an active member of `target_id`.
    pub(crate) async fn require(
        connection: &Connection,
        target_id: &str,
        actor: &Actor,
    ) -> Result<Self> {
        let RoleRow(role_text) = connection
            .query_row::<RoleRow>(
                "SELECT role FROM memberships
                 WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
                (target_id, actor.id.as_str()),
            )
            .await?
            .ok_or_else(|| CollabError::PermissionDenied {
                actor_id: actor.id.clone(),
                action: "participate in",
                target_id: target_id.to_owned(),
            })?;
        Ok(Self {
            role: parse_membership_role(target_id, &actor.id, &role_text)?,
        })
    }

    /// Certify that `actor_id` is the active owner of `target_id`.
    pub(crate) async fn require_owner(
        connection: &Connection,
        target_id: &str,
        actor_id: &str,
    ) -> Result<Self> {
        let role = connection
            .query_row::<RoleRow>(
                "SELECT role FROM memberships
                 WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
                (target_id, actor_id),
            )
            .await?
            .map(|RoleRow(role_text)| parse_membership_role(target_id, actor_id, &role_text))
            .transpose()?;
        let Some(role) = role else {
            return Err(Self::manage_denied(target_id, actor_id));
        };
        if !matches!(role, MembershipRole::Owner) {
            return Err(Self::manage_denied(target_id, actor_id));
        }
        Ok(Self { role })
    }

    pub(crate) const fn role(self) -> MembershipRole {
        self.role
    }

    fn manage_denied(target_id: &str, actor_id: &str) -> CollabError {
        CollabError::PermissionDenied {
            actor_id: actor_id.to_owned(),
            action: "manage",
            target_id: target_id.to_owned(),
        }
    }
}

// ── 能力 ─────────────────────────────────────────────────────────────────────

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
            if actor.kind == crate::ActorKind::Agent {
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

    /// List the local actor directory after authenticating the caller. A
    /// deleted Agent keeps its actors row for message authorship but loses its
    /// agents row, so the directory lists users and live Agents only.
    pub async fn list_actors(&self, actor_id: &str) -> Result<Vec<Actor>> {
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            connection
                .query_rows::<Actor>(
                    "SELECT id, kind, handle, display_name, created_at_ms
                     FROM actors
                     WHERE kind = 'user' OR id IN (SELECT actor_id FROM agents)
                     ORDER BY handle, id",
                    (),
                )
                .await
        })
        .await
    }

    /// List the active members of one Channel after authorizing the caller's
    /// own access to that Channel. This is the membership projection the web
    /// members pane must use; the actor directory is not a member list.
    pub async fn list_target_members(&self, actor_id: &str, target_id: &str) -> Result<Vec<Actor>> {
        self.read(async |connection| {
            let route = require_target_access(connection, target_id, actor_id).await?;
            if route.kind != TargetKind::Channel {
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
        })
        .await
    }

    /// List the role-bearing active roster for an exact target. Thread rosters
    /// are inherited from their parent Channel or Direct target.
    pub async fn list_target_memberships(
        &self,
        actor_id: &str,
        target_id: &str,
    ) -> Result<Vec<TargetMember>> {
        require_non_blank!(actor_id, target_id);
        self.read(async |connection| {
            let route = require_target_access(connection, target_id, actor_id).await?;
            target_memberships(connection, route.permission_target_id(target_id)).await
        })
        .await
    }

    /// List one Agent's active top-level memberships that are also visible to
    /// the requesting actor. Thread membership is inherited and therefore is
    /// not duplicated in this projection.
    pub async fn list_agent_memberships(
        &self,
        actor_id: &str,
        agent_id: &str,
    ) -> Result<Vec<AgentMembership>> {
        require_non_blank!(actor_id, agent_id);
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            ProfileStore::new(connection)
                .require_profile(agent_id)
                .await?;
            agent_memberships_for(connection, actor_id, agent_id).await
        })
        .await
    }
}

// ── 存储 ─────────────────────────────────────────────────────────────────────

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
    require_non_blank!(target_id);
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
