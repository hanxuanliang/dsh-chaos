//! Runtime binding projections.

use crate::{CollabCore, CollabError, Result, RuntimeBinding};

use super::store::{all_bindings, binding_for_agent, binding_for_session};

impl CollabCore {
    /// Return the current runtime binding for one stable Agent.
    pub async fn runtime_binding(&self, agent_id: &str) -> Result<Option<RuntimeBinding>> {
        CollabError::require_non_blank("agent_id", agent_id)?;
        self.read(async |connection| binding_for_agent(connection, agent_id).await)
            .await
    }

    /// Resolve the stable Agent identity that owns one live DSH Session id.
    pub async fn runtime_binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RuntimeBinding>> {
        CollabError::require_non_blank("session_id", session_id)?;
        self.read(async |connection| binding_for_session(connection, session_id).await)
            .await
    }

    /// List every durable current runtime binding for process recovery.
    pub async fn list_runtime_bindings(&self) -> Result<Vec<RuntimeBinding>> {
        self.read(all_bindings).await
    }
}
