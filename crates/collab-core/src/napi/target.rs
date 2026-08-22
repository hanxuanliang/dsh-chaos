//! NAPI bridge for the target vertical.

use crate::Target;
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, to_napi_error};

#[napi(object)]
pub struct JsTarget {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: f64,
}

impl From<Target> for JsTarget {
    fn from(target: Target) -> Self {
        Self {
            id: target.id,
            kind: format!("{:?}", target.kind).to_ascii_lowercase(),
            name: target.name,
            parent_target_id: target.parent_target_id,
            root_message_id: target.root_message_id,
            created_by: target.created_by,
            created_at_ms: target.created_at_ms as f64,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_channel(&self, name: String, creator_id: String) -> Result<JsTarget> {
        self.core
            .create_channel(&name, &creator_id)
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn create_direct(&self, actor_id: String, peer_id: String) -> Result<JsTarget> {
        self.core
            .create_direct(&actor_id, &peer_id)
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }
}
