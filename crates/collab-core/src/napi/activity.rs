//! NAPI bridge for the activity vertical.

use crate::{ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask};
use napi::Result;
use napi_derive::napi;

use super::{CollabHandle, parse_i64, to_napi_error};

#[napi(object)]
pub struct JsActivityInboxReply {
    pub sender_name: String,
    pub sender_kind: String,
    pub excerpt: String,
    pub at_ms: f64,
}

impl From<ActivityInboxReply> for JsActivityInboxReply {
    fn from(reply: ActivityInboxReply) -> Self {
        Self {
            sender_name: reply.sender_name,
            sender_kind: reply.sender_kind.as_str().into(),
            excerpt: reply.excerpt,
            at_ms: reply.at_ms as f64,
        }
    }
}

#[napi(object)]
pub struct JsActivityInboxTask {
    pub number: String,
    pub status: String,
    pub assignee_name: Option<String>,
}

impl From<ActivityInboxTask> for JsActivityInboxTask {
    fn from(task: ActivityInboxTask) -> Self {
        Self {
            number: task.number.to_string(),
            status: task.status.as_str().into(),
            assignee_name: task.assignee_name,
        }
    }
}

#[napi(object)]
pub struct JsActivityInboxItem {
    pub conversation_id: String,
    pub target_kind: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub target_name: String,
    pub title_kind: String,
    pub title: String,
    pub latest_reply: Option<JsActivityInboxReply>,
    pub last_activity_at_ms: f64,
    pub last_activity_seq: String,
    pub reply_count: Option<String>,
    pub task: Option<JsActivityInboxTask>,
    pub done: bool,
}

impl From<ActivityInboxItem> for JsActivityInboxItem {
    fn from(item: ActivityInboxItem) -> Self {
        Self {
            conversation_id: item.conversation_id,
            target_kind: item.target_kind.as_str().into(),
            parent_target_id: item.parent_target_id,
            root_message_id: item.root_message_id,
            target_name: item.target_name,
            title_kind: item.title_kind.as_str().into(),
            title: item.title,
            latest_reply: item.latest_reply.map(JsActivityInboxReply::from),
            last_activity_at_ms: item.last_activity_at_ms as f64,
            last_activity_seq: item.last_activity_seq.to_string(),
            reply_count: item.reply_count.map(|count| count.to_string()),
            task: item.task.map(JsActivityInboxTask::from),
            done: item.done,
        }
    }
}

#[napi(object)]
pub struct JsActivityInboxPage {
    pub items: Vec<JsActivityInboxItem>,
    pub next_cursor: Option<String>,
    pub active_count: String,
}

impl From<ActivityInboxPage> for JsActivityInboxPage {
    fn from(page: ActivityInboxPage) -> Self {
        Self {
            items: page
                .items
                .into_iter()
                .map(JsActivityInboxItem::from)
                .collect(),
            next_cursor: page.next_cursor,
            active_count: page.active_count.to_string(),
        }
    }
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn inbox_list(
        &self,
        actor_id: String,
        limit: u32,
        cursor: Option<String>,
        filter: Option<String>,
    ) -> Result<JsActivityInboxPage> {
        self.core
            .inbox_list(&actor_id, limit, cursor.as_deref(), filter.as_deref())
            .await
            .map(JsActivityInboxPage::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn inbox_done_all(&self, actor_id: String) -> Result<f64> {
        self.core
            .inbox_done_all(&actor_id)
            .await
            .map(f64::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn inbox_done(
        &self,
        actor_id: String,
        target_id: String,
        through_seq: String,
    ) -> Result<()> {
        let through_seq = parse_i64("through_seq", &through_seq)?;
        self.core
            .inbox_done(&actor_id, &target_id, through_seq)
            .await
            .map_err(to_napi_error)
    }
}
