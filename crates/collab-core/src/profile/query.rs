use crate::actor::ActorId;
use crate::membership::identity_context_for;
use crate::{Actor, AgentProfile, CollabCore, CollabError, IdentityContext, Result};

use super::store::ProfileStore;

impl CollabCore {
    /// Read one active stable Agent Profile independently from its DSH Session.
    pub async fn agent_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        CollabError::require_non_blank("agent_id", agent_id)?;
        self.read(async |connection| {
            ProfileStore::new(connection)
                .require_profile(agent_id)
                .await
        })
        .await
    }

    /// List every live Agent Profile in one coherent directory read.
    pub async fn list_agent_profiles(&self, actor_id: &str) -> Result<Vec<AgentProfile>> {
        CollabError::require_non_blank("actor_id", actor_id)?;
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            ProfileStore::new(connection).directory().await
        })
        .await
    }

    /// Return the caller's stable identity and optional exact target roster.
    pub async fn identity_context(
        &self,
        agent_id: &str,
        target_id: Option<&str>,
    ) -> Result<IdentityContext> {
        CollabError::require_non_blank("agent_id", agent_id)?;
        self.read(async |connection| identity_context_for(connection, agent_id, target_id).await)
            .await
    }
}
