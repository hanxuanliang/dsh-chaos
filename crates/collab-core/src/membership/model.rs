use serde::{Deserialize, Serialize};
use turso::{Connection, Row};

use crate::db::{FromRow, QueryRows};
use crate::{Actor, AgentProfile, CollabError, Result, Target};

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

    pub(crate) const fn is_owner(self) -> bool {
        matches!(self.role, MembershipRole::Owner)
    }

    fn manage_denied(target_id: &str, actor_id: &str) -> CollabError {
        CollabError::PermissionDenied {
            actor_id: actor_id.to_owned(),
            action: "manage",
            target_id: target_id.to_owned(),
        }
    }
}
