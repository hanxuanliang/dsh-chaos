//! NAPI bridge for the target vertical.

use crate::Target;
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct JsTarget {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub lifecycle: String,
    pub version: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: f64,
    pub updated_at_ms: f64,
    pub archived_at_ms: Option<f64>,
    pub deleted_at_ms: Option<f64>,
}

impl From<Target> for JsTarget {
    fn from(target: Target) -> Self {
        Self {
            id: target.id,
            kind: format!("{:?}", target.kind).to_ascii_lowercase(),
            name: target.name,
            description: target.description,
            lifecycle: target.lifecycle.as_str().into(),
            version: target.version.to_string(),
            parent_target_id: target.parent_target_id,
            root_message_id: target.root_message_id,
            created_by: target.created_by,
            created_at_ms: target.created_at_ms as f64,
            updated_at_ms: target.updated_at_ms as f64,
            archived_at_ms: target.archived_at_ms.map(|value| value as f64),
            deleted_at_ms: target.deleted_at_ms.map(|value| value as f64),
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_channel(
        &self,
        name: String,
        description: String,
        creator_id: String,
    ) -> Result<JsTarget> {
        self.core
            .create_channel(&name, &description, &creator_id)
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_channel(
        &self,
        target_id: String,
        actor_id: String,
        name: String,
        description: String,
        expected_version: String,
    ) -> Result<JsTarget> {
        self.core
            .update_channel(
                &target_id,
                &actor_id,
                &name,
                &description,
                parse_i64("expected_version", &expected_version)?,
            )
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn archive_channel(
        &self,
        target_id: String,
        actor_id: String,
        expected_version: String,
    ) -> Result<JsTarget> {
        self.core
            .archive_channel(
                &target_id,
                &actor_id,
                parse_i64("expected_version", &expected_version)?,
            )
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn restore_channel(
        &self,
        target_id: String,
        actor_id: String,
        expected_version: String,
    ) -> Result<JsTarget> {
        self.core
            .restore_channel(
                &target_id,
                &actor_id,
                parse_i64("expected_version", &expected_version)?,
            )
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn delete_channel(
        &self,
        target_id: String,
        actor_id: String,
        expected_version: String,
    ) -> Result<JsTarget> {
        self.core
            .delete_channel(
                &target_id,
                &actor_id,
                parse_i64("expected_version", &expected_version)?,
            )
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
