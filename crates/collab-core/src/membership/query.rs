//! Actor directory and member roster projections.

use crate::actor::ActorId;
use crate::db::QueryRows;
use crate::profile::store::ProfileStore;
use crate::target::require_target_access;
use crate::{Actor, AgentMembership, CollabCore, CollabError, Result, TargetKind, TargetMember};

use super::store::{agent_memberships_for, target_memberships};

impl CollabCore {
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
        CollabError::require_non_blank("actor_id", actor_id)?;
        CollabError::require_non_blank("target_id", target_id)?;
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
        CollabError::require_non_blank("actor_id", actor_id)?;
        CollabError::require_non_blank("agent_id", agent_id)?;
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
