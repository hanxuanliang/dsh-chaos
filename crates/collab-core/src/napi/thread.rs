//! NAPI bridge for the thread vertical.

use crate::ThreadSummary;
use napi::Result;
use napi_derive::napi;

use super::target::JsTarget;
use super::{CollabHandle, to_napi_error};

#[napi(object)]
pub struct JsThreadSummary {
    pub root_message_id: String,
    pub thread_id: String,
    pub reply_count: f64,
    pub last_reply_at_ms: Option<f64>,
    pub recent_replier_ids: Vec<String>,
}

impl From<ThreadSummary> for JsThreadSummary {
    fn from(summary: ThreadSummary) -> Self {
        Self {
            root_message_id: summary.root_message_id,
            thread_id: summary.thread_id,
            reply_count: summary.reply_count as f64,
            last_reply_at_ms: summary.last_reply_at_ms.map(|ms| ms as f64),
            recent_replier_ids: summary.recent_replier_ids,
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn create_thread(
        &self,
        root_message_id: String,
        actor_id: String,
    ) -> Result<JsTarget> {
        self.core
            .create_thread(&root_message_id, &actor_id)
            .await
            .map(JsTarget::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn thread_summaries(
        &self,
        actor_id: String,
        root_message_ids: Vec<String>,
    ) -> Result<Vec<JsThreadSummary>> {
        self.core
            .thread_summaries(&actor_id, &root_message_ids)
            .await
            .map(|summaries| summaries.into_iter().map(JsThreadSummary::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn follow_thread(&self, thread_target_id: String, actor_id: String) -> Result<()> {
        self.core
            .follow_thread(&thread_target_id, &actor_id)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn unfollow_thread(&self, thread_target_id: String, actor_id: String) -> Result<()> {
        self.core
            .unfollow_thread(&thread_target_id, &actor_id)
            .await
            .map_err(to_napi_error)
    }
}
