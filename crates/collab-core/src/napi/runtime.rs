//! NAPI bridge for the runtime vertical.

use crate::RuntimeBinding;
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, to_napi_error};

#[napi(object)]
pub struct JsRuntimeBinding {
    pub agent_id: String,
    pub session_id: String,
    pub generation: String,
    pub provider: String,
    pub model: String,
    pub preset: String,
    pub bound_at_ms: f64,
}

impl From<RuntimeBinding> for JsRuntimeBinding {
    fn from(binding: RuntimeBinding) -> Self {
        Self {
            agent_id: binding.agent_id,
            session_id: binding.session_id,
            generation: binding.generation.to_string(),
            provider: binding.provider,
            model: binding.model,
            preset: binding.preset,
            bound_at_ms: binding.bound_at_ms as f64,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn bind_runtime(
        &self,
        agent_id: String,
        session_id: String,
        provider: String,
        model: String,
        preset: String,
    ) -> Result<JsRuntimeBinding> {
        self.core
            .bind_runtime(&agent_id, &session_id, &provider, &model, &preset)
            .await
            .map(JsRuntimeBinding::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn runtime_binding(&self, agent_id: String) -> Result<Option<JsRuntimeBinding>> {
        self.core
            .runtime_binding(&agent_id)
            .await
            .map(|binding| binding.map(JsRuntimeBinding::from))
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_runtime_preset(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
        preset: String,
    ) -> Result<JsRuntimeBinding> {
        let generation = generation
            .parse::<i64>()
            .map_err(|_| napi::Error::from_reason("generation must be a decimal integer"))?;
        self.core
            .update_runtime_preset(&agent_id, generation, &session_id, &preset)
            .await
            .map(JsRuntimeBinding::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn runtime_binding_for_session(
        &self,
        session_id: String,
    ) -> Result<Option<JsRuntimeBinding>> {
        self.core
            .runtime_binding_for_session(&session_id)
            .await
            .map(|binding| binding.map(JsRuntimeBinding::from))
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_runtime_bindings(&self) -> Result<Vec<JsRuntimeBinding>> {
        self.core
            .list_runtime_bindings()
            .await
            .map(|bindings| bindings.into_iter().map(JsRuntimeBinding::from).collect())
            .map_err(to_napi_error)
    }
}
