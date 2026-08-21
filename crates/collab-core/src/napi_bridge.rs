//! Coarse-grained NAPI bridge for the Rust collab core.

use std::sync::Arc;

use crate::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, Actor,
    AgentCharter, AgentMembership, AgentProfile, ChangeEvent, CollabCore, CollabError,
    CollabSnapshot, IdentityContext, InboxBatch, Message, PendingWake, RuntimeBinding,
    SendMessageRequest, SendMessageResult, Target, TargetMember, Task, TaskStatus, ThreadSummary,
};
use napi::{Error, Result, Status};
use napi_derive::napi;

#[napi]
pub struct CollabHandle {
    core: Arc<CollabCore>,
}

#[napi(object)]
pub struct SendMessageInput {
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
}

#[napi(object)]
pub struct JsActor {
    pub id: String,
    pub kind: String,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsAgentCharter {
    pub schema_version: u32,
    pub summary: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
}

#[napi(object)]
pub struct JsAgentProfile {
    pub actor: JsActor,
    pub workspace_path: String,
    pub lifecycle: String,
    pub charter: JsAgentCharter,
    pub version: String,
    pub created_at_ms: f64,
    pub updated_at_ms: f64,
}

#[napi(object)]
pub struct JsTargetMember {
    pub actor: JsActor,
    pub role: String,
    pub joined_at_ms: f64,
}

#[napi(object)]
pub struct JsAgentMembership {
    pub target: JsTarget,
    pub role: String,
    pub joined_at_ms: f64,
}

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

#[napi(object)]
pub struct JsIdentityContext {
    pub agent: JsAgentProfile,
    pub target: Option<JsTarget>,
    pub membership_target: Option<JsTarget>,
    pub members: Vec<JsTargetMember>,
}

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

#[napi(object)]
pub struct JsMessage {
    pub seq: String,
    pub id: String,
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsMessageTail {
    /// Exact total message count of the target, as a decimal string so
    /// JavaScript never loses integer precision.
    pub count: String,
    pub messages: Vec<JsMessage>,
}

#[napi(object)]
pub struct JsActivityInboxReply {
    pub sender_name: String,
    pub sender_kind: String,
    pub excerpt: String,
    pub at_ms: f64,
}

#[napi(object)]
pub struct JsActivityInboxTask {
    pub number: String,
    pub status: String,
    pub assignee_name: Option<String>,
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
}

#[napi(object)]
pub struct JsActivityInboxPage {
    pub items: Vec<JsActivityInboxItem>,
    pub next_cursor: Option<String>,
    pub active_count: String,
}

#[napi(object)]
pub struct JsSendMessageResult {
    pub message: JsMessage,
    pub recipient_ids: Vec<String>,
    pub wake_agent_ids: Vec<String>,
    pub replayed: bool,
}

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

#[napi(object)]
pub struct JsPendingWake {
    pub binding: JsRuntimeBinding,
    pub pending_seq: String,
}

#[napi(object)]
pub struct JsInboxMessage {
    pub delivery_id: String,
    pub message: JsMessage,
}

#[napi(object)]
pub struct JsInboxBatch {
    pub id: Option<String>,
    pub agent_id: String,
    pub session_id: String,
    pub generation: String,
    pub messages: Vec<JsInboxMessage>,
    pub contexts: Vec<JsIdentityContext>,
    pub checked_at_ms: f64,
}

#[napi(object)]
pub struct JsTask {
    pub message_id: String,
    pub target_id: String,
    pub number: String,
    pub status: String,
    pub assignee_id: Option<String>,
    pub version: String,
    pub created_at_ms: f64,
    pub updated_at_ms: f64,
    pub anchor_text: Option<String>,
}

#[napi(object)]
pub struct JsChangeEvent {
    pub seq: String,
    pub kind: String,
    pub target_id: Option<String>,
    pub entity_id: String,
    pub created_at_ms: f64,
}

#[napi(object)]
pub struct JsCollabSnapshot {
    pub actor: JsActor,
    pub cursor: String,
    pub targets: Vec<JsTarget>,
    pub followed_thread_ids: Vec<String>,
    pub tasks: Vec<JsTask>,
}

#[napi]
pub async fn open_collab(path: String) -> Result<CollabHandle> {
    let core = CollabCore::open(path).await.map_err(to_napi_error)?;
    Ok(CollabHandle {
        core: Arc::new(core),
    })
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn close(&self) -> Result<()> {
        self.core.close().await.map_err(to_napi_error)
    }

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
    pub async fn create_agent_profile(
        &self,
        handle: String,
        display_name: String,
        workspace_path: String,
        charter: JsAgentCharter,
    ) -> Result<JsAgentProfile> {
        self.core
            .create_agent_profile(&handle, &display_name, &workspace_path, charter.into())
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn agent_profile(&self, agent_id: String) -> Result<JsAgentProfile> {
        self.core
            .agent_profile(&agent_id)
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_agent_profiles(&self, actor_id: String) -> Result<Vec<JsAgentProfile>> {
        self.core
            .list_agent_profiles(&actor_id)
            .await
            .map(|profiles| profiles.into_iter().map(JsAgentProfile::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_agent_profile(
        &self,
        agent_id: String,
        display_name: String,
        charter: JsAgentCharter,
        expected_version: String,
    ) -> Result<JsAgentProfile> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        self.core
            .update_agent_profile(&agent_id, &display_name, charter.into(), expected_version)
            .await
            .map(JsAgentProfile::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn identity_context(
        &self,
        agent_id: String,
        target_id: Option<String>,
    ) -> Result<JsIdentityContext> {
        self.core
            .identity_context(&agent_id, target_id.as_deref())
            .await
            .map(JsIdentityContext::from)
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

    #[napi]
    pub async fn add_member(
        &self,
        target_id: String,
        actor_id: String,
        added_by: String,
    ) -> Result<()> {
        self.core
            .add_member(&target_id, &actor_id, &added_by)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn send_message(&self, input: SendMessageInput) -> Result<JsSendMessageResult> {
        self.core
            .send_message(SendMessageRequest {
                target_id: input.target_id,
                author_id: input.author_id,
                client_request_id: input.client_request_id,
                text: input.text,
            })
            .await
            .map(JsSendMessageResult::from)
            .map_err(to_napi_error)
    }

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

    #[napi]
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<JsPendingWake>> {
        self.core
            .list_pending_wakes(limit)
            .await
            .map(|wakes| wakes.into_iter().map(JsPendingWake::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn mark_notified(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
        pending_seq: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        let pending_seq = parse_i64("pending_seq", &pending_seq)?;
        self.core
            .mark_notified(&agent_id, generation, &session_id, pending_seq)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn rearm_runtime_wake(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .rearm_runtime_wake(&agent_id, generation, &session_id)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn check_inbox(
        &self,
        agent_id: String,
        generation: String,
        session_id: String,
        limit: u32,
    ) -> Result<JsInboxBatch> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .check_inbox(&agent_id, generation, &session_id, limit)
            .await
            .map(JsInboxBatch::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn mark_model_seen(
        &self,
        batch_id: String,
        agent_id: String,
        generation: String,
        session_id: String,
    ) -> Result<()> {
        let generation = parse_i64("generation", &generation)?;
        self.core
            .mark_model_seen(&batch_id, &agent_id, generation, &session_id)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn read_message(
        &self,
        actor_id: String,
        target_id: String,
        message_id: String,
    ) -> Result<JsMessage> {
        self.core
            .read_message(&actor_id, &target_id, &message_id)
            .await
            .map(JsMessage::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn read_messages(
        &self,
        actor_id: String,
        target_id: String,
        after_seq: String,
        limit: u32,
    ) -> Result<Vec<JsMessage>> {
        let after_seq = parse_i64("after_seq", &after_seq)?;
        self.core
            .read_messages(&actor_id, &target_id, after_seq, limit)
            .await
            .map(|messages| messages.into_iter().map(JsMessage::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn read_messages_tail(
        &self,
        actor_id: String,
        target_id: String,
        limit: u32,
    ) -> Result<JsMessageTail> {
        self.core
            .read_messages_tail(&actor_id, &target_id, limit)
            .await
            .map(|tail| JsMessageTail {
                count: tail.count.to_string(),
                messages: tail.messages.into_iter().map(JsMessage::from).collect(),
            })
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn inbox_list(
        &self,
        actor_id: String,
        limit: u32,
        cursor: Option<String>,
    ) -> Result<JsActivityInboxPage> {
        self.core
            .inbox_list(&actor_id, limit, cursor.as_deref())
            .await
            .map(JsActivityInboxPage::from)
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

    #[napi]
    pub async fn list_actors(&self, actor_id: String) -> Result<Vec<JsActor>> {
        self.core
            .list_actors(&actor_id)
            .await
            .map(|actors| actors.into_iter().map(JsActor::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_target_members(
        &self,
        actor_id: String,
        target_id: String,
    ) -> Result<Vec<JsActor>> {
        self.core
            .list_target_members(&actor_id, &target_id)
            .await
            .map(|members| members.into_iter().map(JsActor::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_target_memberships(
        &self,
        actor_id: String,
        target_id: String,
    ) -> Result<Vec<JsTargetMember>> {
        self.core
            .list_target_memberships(&actor_id, &target_id)
            .await
            .map(|members| members.into_iter().map(JsTargetMember::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_agent_memberships(
        &self,
        actor_id: String,
        agent_id: String,
    ) -> Result<Vec<JsAgentMembership>> {
        self.core
            .list_agent_memberships(&actor_id, &agent_id)
            .await
            .map(|memberships| {
                memberships
                    .into_iter()
                    .map(JsAgentMembership::from)
                    .collect()
            })
            .map_err(to_napi_error)
    }

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

    #[napi]
    pub async fn create_task(&self, message_id: String, actor_id: String) -> Result<JsTask> {
        self.core
            .create_task(&message_id, &actor_id)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn claim_task(&self, message_id: String, actor_id: String) -> Result<JsTask> {
        self.core
            .claim_task(&message_id, &actor_id)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn list_tasks(
        &self,
        actor_id: String,
        target_id: Option<String>,
    ) -> Result<Vec<JsTask>> {
        self.core
            .list_tasks(&actor_id, target_id.as_deref())
            .await
            .map(|tasks| tasks.into_iter().map(JsTask::from).collect())
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn unclaim_task(
        &self,
        message_id: String,
        actor_id: String,
        expected_version: String,
    ) -> Result<JsTask> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        self.core
            .unclaim_task(&message_id, &actor_id, expected_version)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn update_task_status(
        &self,
        message_id: String,
        actor_id: String,
        status: String,
        expected_version: String,
    ) -> Result<JsTask> {
        let expected_version = parse_i64("expected_version", &expected_version)?;
        let status = TaskStatus::parse(&status).ok_or_else(|| {
            Error::new(
                Status::InvalidArg,
                "[invalid_argument] status must be todo, in_progress, in_review, or done",
            )
        })?;
        self.core
            .update_task_status(&message_id, &actor_id, status, expected_version)
            .await
            .map(JsTask::from)
            .map_err(to_napi_error)
    }
}

impl From<Actor> for JsActor {
    fn from(actor: Actor) -> Self {
        Self {
            id: actor.id,
            kind: format!("{:?}", actor.kind).to_ascii_lowercase(),
            handle: actor.handle,
            display_name: actor.display_name,
            created_at_ms: actor.created_at_ms as f64,
        }
    }
}

impl From<JsAgentCharter> for AgentCharter {
    fn from(charter: JsAgentCharter) -> Self {
        Self {
            schema_version: charter.schema_version,
            summary: charter.summary,
            capabilities: charter.capabilities,
            constraints: charter.constraints,
        }
    }
}

impl From<AgentCharter> for JsAgentCharter {
    fn from(charter: AgentCharter) -> Self {
        Self {
            schema_version: charter.schema_version,
            summary: charter.summary,
            capabilities: charter.capabilities,
            constraints: charter.constraints,
        }
    }
}

impl From<AgentProfile> for JsAgentProfile {
    fn from(profile: AgentProfile) -> Self {
        Self {
            actor: profile.actor.into(),
            workspace_path: profile.workspace_path,
            lifecycle: profile.lifecycle.as_str().into(),
            charter: profile.charter.into(),
            version: profile.version.to_string(),
            created_at_ms: profile.created_at_ms as f64,
            updated_at_ms: profile.updated_at_ms as f64,
        }
    }
}

impl From<TargetMember> for JsTargetMember {
    fn from(member: TargetMember) -> Self {
        Self {
            actor: member.actor.into(),
            role: member.role.as_str().into(),
            joined_at_ms: member.joined_at_ms as f64,
        }
    }
}

impl From<AgentMembership> for JsAgentMembership {
    fn from(membership: AgentMembership) -> Self {
        Self {
            target: membership.target.into(),
            role: membership.role.as_str().into(),
            joined_at_ms: membership.joined_at_ms as f64,
        }
    }
}

impl From<IdentityContext> for JsIdentityContext {
    fn from(context: IdentityContext) -> Self {
        Self {
            agent: context.agent.into(),
            target: context.target.map(JsTarget::from),
            membership_target: context.membership_target.map(JsTarget::from),
            members: context
                .members
                .into_iter()
                .map(JsTargetMember::from)
                .collect(),
        }
    }
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

impl From<Message> for JsMessage {
    fn from(message: Message) -> Self {
        Self {
            seq: message.seq.to_string(),
            id: message.id,
            target_id: message.target_id,
            author_id: message.author_id,
            client_request_id: message.client_request_id,
            text: message.text,
            created_at_ms: message.created_at_ms as f64,
        }
    }
}

impl From<SendMessageResult> for JsSendMessageResult {
    fn from(result: SendMessageResult) -> Self {
        Self {
            message: result.message.into(),
            recipient_ids: result.recipient_ids,
            wake_agent_ids: result.wake_agent_ids,
            replayed: result.replayed,
        }
    }
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

impl From<ActivityInboxTask> for JsActivityInboxTask {
    fn from(task: ActivityInboxTask) -> Self {
        Self {
            number: task.number.to_string(),
            status: task.status.as_str().into(),
            assignee_name: task.assignee_name,
        }
    }
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
        }
    }
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

impl From<PendingWake> for JsPendingWake {
    fn from(wake: PendingWake) -> Self {
        Self {
            binding: wake.binding.into(),
            pending_seq: wake.pending_seq.to_string(),
        }
    }
}

impl From<InboxBatch> for JsInboxBatch {
    fn from(batch: InboxBatch) -> Self {
        Self {
            id: batch.id,
            agent_id: batch.agent_id,
            session_id: batch.session_id,
            generation: batch.generation.to_string(),
            messages: batch
                .messages
                .into_iter()
                .map(|item| JsInboxMessage {
                    delivery_id: item.delivery_id,
                    message: item.message.into(),
                })
                .collect(),
            contexts: batch
                .contexts
                .into_iter()
                .map(JsIdentityContext::from)
                .collect(),
            checked_at_ms: batch.checked_at_ms as f64,
        }
    }
}

impl From<Task> for JsTask {
    fn from(task: Task) -> Self {
        let status = match task.status {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::InReview => "in_review",
            TaskStatus::Done => "done",
        };
        Self {
            message_id: task.message_id,
            target_id: task.target_id,
            number: task.number.to_string(),
            status: status.into(),
            assignee_id: task.assignee_id,
            version: task.version.to_string(),
            created_at_ms: task.created_at_ms as f64,
            updated_at_ms: task.updated_at_ms as f64,
            anchor_text: task.anchor_text,
        }
    }
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

fn parse_i64(name: &str, value: &str) -> Result<i64> {
    value.parse().map_err(|_| {
        Error::new(
            Status::InvalidArg,
            format!("[invalid_argument] {name} must be a signed 64-bit decimal string"),
        )
    })
}

fn parse_millis(name: &str, value: f64) -> Result<i64> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_SAFE_INTEGER {
        return Err(Error::new(
            Status::InvalidArg,
            format!("[invalid_argument] {name} must be a non-negative integer millisecond value"),
        ));
    }
    Ok(value as i64)
}

fn to_napi_error(error: CollabError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("[{}] {error}", error.code()),
    )
}
