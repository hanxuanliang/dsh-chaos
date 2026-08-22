//! NAPI bridge for the changefeed vertical.

use crate::{ChangeEvent, CollabSnapshot};
use napi::Result;
use napi_derive::napi;

use super::actor::JsActor;
use super::target::JsTarget;
use super::task::JsTask;
use super::{CollabHandle, parse_i64, parse_millis, to_napi_error};

#[napi(object)]
pub struct JsChangeEvent {
    pub seq: String,
    pub kind: String,
    pub target_id: Option<String>,
    pub entity_id: String,
    pub created_at_ms: f64,
}

impl From<ChangeEvent> for JsChangeEvent {
    fn from(change: ChangeEvent) -> Self {
        Self {
            seq: change.seq.to_string(),
            kind: change.kind.as_str().into(),
            target_id: change.target_id,
            entity_id: change.entity_id,
            created_at_ms: change.created_at_ms as f64,
        }
    }
}

#[napi(object)]
pub struct JsCollabSnapshot {
    pub actor: JsActor,
    pub cursor: String,
    pub targets: Vec<JsTarget>,
    pub followed_thread_ids: Vec<String>,
    pub tasks: Vec<JsTask>,
}

impl From<CollabSnapshot> for JsCollabSnapshot {
    fn from(snapshot: CollabSnapshot) -> Self {
        Self {
            actor: snapshot.actor.into(),
            cursor: snapshot.cursor.to_string(),
            targets: snapshot.targets.into_iter().map(JsTarget::from).collect(),
            followed_thread_ids: snapshot.followed_thread_ids,
            tasks: snapshot.tasks.into_iter().map(JsTask::from).collect(),
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn snapshot(&self, actor_id: String) -> Result<JsCollabSnapshot> {
        self.core
            .snapshot(&actor_id)
            .await
            .map(JsCollabSnapshot::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_changes(
        &self,
        actor_id: String,
        after_seq: String,
        limit: u32,
    ) -> Result<Vec<JsChangeEvent>> {
        let after_seq = parse_i64("after_seq", &after_seq)?;
        self.core
            .list_changes(&actor_id, after_seq, limit)
            .await
            .map(|changes| changes.into_iter().map(JsChangeEvent::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn prune_changes_before(&self, before_ms: f64) -> Result<String> {
        let before_ms = parse_millis("before_ms", before_ms)?;
        self.core
            .prune_changes_before(before_ms)
            .await
            .map(|floor| floor.to_string())
            .map_err(to_napi_error)
    }
}
