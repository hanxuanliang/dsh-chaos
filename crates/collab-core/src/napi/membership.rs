//! NAPI bridge for the membership vertical.

use crate::{AgentMembership, IdentityContext, TargetMember};
use napi::Result;
use napi_derive::napi;

use super::actor::JsActor;
use super::profile::JsAgentProfile;
use super::target::JsTarget;
use super::{CollabHandle, to_napi_error};

#[napi(object)]
pub struct JsTargetMember {
    pub actor: JsActor,
    pub role: String,
    pub joined_at_ms: f64,
}

impl From<TargetMember> for JsTargetMember {
    fn from(member: TargetMember) -> Self {
        Self {
            actor: member.actor.into(),
            role: member.role.as_str().into(),
            joined_at_ms: member.joined_at_ms as f64,
        }
    }
}

#[napi(object)]
pub struct JsAgentMembership {
    pub target: JsTarget,
    pub role: String,
    pub joined_at_ms: f64,
}

impl From<AgentMembership> for JsAgentMembership {
    fn from(membership: AgentMembership) -> Self {
        Self {
            target: membership.target.into(),
            role: membership.role.as_str().into(),
            joined_at_ms: membership.joined_at_ms as f64,
        }
    }
}

#[napi(object)]
pub struct JsIdentityContext {
    pub agent: JsAgentProfile,
    pub target: Option<JsTarget>,
    pub membership_target: Option<JsTarget>,
    pub members: Vec<JsTargetMember>,
}

impl From<IdentityContext> for JsIdentityContext {
    fn from(context: IdentityContext) -> Self {
        Self {
            agent: context.agent.into(),
            target: context.target.map(JsTarget::from),
            membership_target: context.membership_target.map(JsTarget::from),
            members: context
                .members
                .into_iter()
                .map(JsTargetMember::from)
                .collect(),
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn add_member(
        &self,
        target_id: String,
        actor_id: String,
        added_by: String,
    ) -> Result<()> {
        self.core
            .add_member(&target_id, &actor_id, &added_by)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn identity_context(
        &self,
        agent_id: String,
        target_id: Option<String>,
    ) -> Result<JsIdentityContext> {
        self.core
            .identity_context(&agent_id, target_id.as_deref())
            .await
            .map(JsIdentityContext::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_target_members(
        &self,
        actor_id: String,
        target_id: String,
    ) -> Result<Vec<JsActor>> {
        self.core
            .list_target_members(&actor_id, &target_id)
            .await
            .map(|members| members.into_iter().map(JsActor::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_target_memberships(
        &self,
        actor_id: String,
        target_id: String,
    ) -> Result<Vec<JsTargetMember>> {
        self.core
            .list_target_memberships(&actor_id, &target_id)
            .await
            .map(|members| members.into_iter().map(JsTargetMember::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_agent_memberships(
        &self,
        actor_id: String,
        agent_id: String,
    ) -> Result<Vec<JsAgentMembership>> {
        self.core
            .list_agent_memberships(&actor_id, &agent_id)
            .await
            .map(|memberships| {
                memberships
                    .into_iter()
                    .map(JsAgentMembership::from)
                    .collect()
            })
            .map_err(to_napi_error)
    }
}
