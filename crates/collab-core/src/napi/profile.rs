//! NAPI bridge for the agent profile vertical.

use crate::{AgentCharter, AgentProfile};
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct JsAgentCharter {
    pub schema_version: u32,
    pub summary: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
}

impl From<JsAgentCharter> for AgentCharter {
    fn from(charter: JsAgentCharter) -> Self {
        Self {
            schema_version: charter.schema_version,
            summary: charter.summary,
            capabilities: charter.capabilities,
            constraints: charter.constraints,
        }
    }
}

impl From<AgentCharter> for JsAgentCharter {
    fn from(charter: AgentCharter) -> Self {
        Self {
            schema_version: charter.schema_version,
            summary: charter.summary,
            capabilities: charter.capabilities,
            constraints: charter.constraints,
        }
    }
}

#[napi(object)]
pub struct JsAgentProfile {
    pub actor: super::actor::JsActor,
    pub workspace_path: String,
    pub lifecycle: String,
    pub charter: JsAgentCharter,
    pub version: String,
    pub created_at_ms: f64,
    pub updated_at_ms: f64,
}

impl From<AgentProfile> for JsAgentProfile {
    fn from(profile: AgentProfile) -> Self {
        Self {
            actor: profile.actor.into(),
            workspace_path: profile.workspace_path,
            lifecycle: profile.lifecycle.as_str().into(),
            charter: profile.charter.into(),
            version: profile.version.to_string(),
            created_at_ms: profile.created_at_ms as f64,
            updated_at_ms: profile.updated_at_ms as f64,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_agent_profile(
        &self,
        handle: String,
        display_name: String,
        workspace_path: String,
        charter: JsAgentCharter,
    ) -> Result<JsAgentProfile> {
        self.core
            .create_agent_profile(&handle, &display_name, &workspace_path, charter.into())
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn agent_profile(&self, agent_id: String) -> Result<JsAgentProfile> {
        self.core
            .agent_profile(&agent_id)
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_agent_profiles(&self, actor_id: String) -> Result<Vec<JsAgentProfile>> {
        self.core
            .list_agent_profiles(&actor_id)
            .await
            .map(|profiles| profiles.into_iter().map(JsAgentProfile::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_agent_profile(
        &self,
        agent_id: String,
        display_name: String,
        charter: JsAgentCharter,
        expected_version: String,
    ) -> Result<JsAgentProfile> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        self.core
            .update_agent_profile(&agent_id, &display_name, charter.into(), expected_version)
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_agent_avatar(
        &self,
        agent_id: String,
        avatar_data_url: Option<String>,
        expected_version: String,
    ) -> Result<JsAgentProfile> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        self.core
            .update_agent_avatar(&agent_id, avatar_data_url.as_deref(), expected_version)
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }
}
