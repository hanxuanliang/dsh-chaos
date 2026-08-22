//! Runtime binding mutation.

use crate::actor::{Actor, ActorId};
use crate::{CollabCore, CollabError, Result, RuntimeBinding, now_ms};

use super::store::{binding_for_agent, current_generation};

impl CollabCore {
    /// Bind a new DSH Session generation to a stable Agent.
    pub async fn bind_runtime(
        &self,
        agent_id: &str,
        session_id: &str,
        provider: &str,
        model: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        for (name, value) in [
            ("agent_id", agent_id),
            ("session_id", session_id),
            ("provider", provider),
            ("model", model),
            ("preset", preset),
        ] {
            CollabError::require_non_blank(name, value)?;
        }
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::require_agent(connection, &ActorId::parse(agent_id)?).await?;
            let generation = current_generation(connection, agent_id).await? + 1;
            let binding = RuntimeBinding {
                agent_id: agent_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
                provider: provider.to_owned(),
                model: model.to_owned(),
                preset: preset.to_owned(),
                bound_at_ms: now,
            };
            binding.upsert(connection).await?;
            Ok(binding)
        })
        .await
    }

    /// Replace only the preset label of one exact runtime generation.
    ///
    /// Used to migrate the former chaos `default` sentinel after the official
    /// DSH roster resolves it. Session identity, generation, and wake fencing
    /// stay unchanged.
    pub async fn update_runtime_preset(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        CollabError::require_non_blank("agent_id", agent_id)?;
        CollabError::require_non_blank("session_id", session_id)?;
        CollabError::require_non_blank("preset", preset)?;
        self.write(async |connection| {
            let current = binding_for_agent(connection, agent_id)
                .await?
                .ok_or_else(|| CollabError::NotFound {
                    entity: "runtime binding",
                    id: agent_id.to_owned(),
                })?;
            if current.generation != generation || current.session_id != session_id {
                return Err(CollabError::RuntimeGenerationMismatch {
                    agent_id: agent_id.to_owned(),
                });
            }
            current.update_preset(connection, preset).await?;
            Ok(RuntimeBinding {
                preset: preset.to_owned(),
                ..current
            })
        })
        .await
    }
}
