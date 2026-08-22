//! NAPI bridge for the actor vertical.

use crate::Actor;
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, to_napi_error};

#[napi(object)]
pub struct JsActor {
    pub id: String,
    pub kind: String,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: f64,
    pub avatar_data_url: Option<String>,
}

impl From<Actor> for JsActor {
    fn from(actor: Actor) -> Self {
        Self {
            id: actor.id,
            kind: format!("{:?}", actor.kind).to_ascii_lowercase(),
            handle: actor.handle,
            display_name: actor.display_name,
            created_at_ms: actor.created_at_ms as f64,
            avatar_data_url: actor.avatar_data_url,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_user(&self, handle: String, display_name: String) -> Result<JsActor> {
        self.core
            .create_user(&handle, &display_name)
            .await
            .map(JsActor::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn ensure_user(&self, handle: String, display_name: String) -> Result<JsActor> {
        self.core
            .ensure_user(&handle, &display_name)
            .await
            .map(JsActor::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn create_agent(
        &self,
        handle: String,
        display_name: String,
        workspace_path: String,
    ) -> Result<JsActor> {
        self.core
            .create_agent(&handle, &display_name, &workspace_path)
            .await
            .map(JsActor::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn delete_agent(&self, actor_id: String) -> Result<()> {
        self.core
            .delete_agent(&actor_id)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_actors(&self, actor_id: String) -> Result<Vec<JsActor>> {
        self.core
            .list_actors(&actor_id)
            .await
            .map(|actors| actors.into_iter().map(JsActor::from).collect())
            .map_err(to_napi_error)
    }
}
