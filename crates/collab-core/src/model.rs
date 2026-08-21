use serde::{Deserialize, Serialize};

/// A stable collab actor kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
}

impl ActorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

/// A stable collab actor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
    pub handle: String,
    pub display_name: String,
    pub created_at_ms: i64,
}

/// Versioned, stable collaboration responsibilities for one Agent.
///
/// The Rust type is the contract; storage uses canonical JSON so future schema
/// versions can add bounded fields without turning the charter into a free-form
/// key/value bag.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCharter {
    pub schema_version: u32,
    pub summary: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
}

impl Default for AgentCharter {
    fn default() -> Self {
        Self {
            schema_version: 1,
            summary: String::new(),
            capabilities: Vec::new(),
            constraints: Vec::new(),
        }
    }
}

/// Operational lifecycle of a stable Agent Profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Active,
    Archived,
}

impl AgentLifecycle {
    #[cfg(feature = "napi")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

/// Stable Agent identity and workspace state, independent from its DSH Session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentProfile {
    pub actor: Actor,
    pub workspace_path: String,
    pub lifecycle: AgentLifecycle,
    pub charter: AgentCharter,
    pub version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Role of one active target member.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    Owner,
    Member,
}

impl MembershipRole {
    #[cfg(feature = "napi")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }
}

/// One role-bearing target member, enriched with its stable human-readable identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TargetMember {
    pub actor: Actor,
    pub role: MembershipRole,
    pub joined_at_ms: i64,
}

/// A collab target kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Channel,
    Direct,
    Thread,
}

impl TargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Direct => "direct",
            Self::Thread => "thread",
        }
    }
}

/// A stable exact collab target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Target {
    pub id: String,
    pub kind: TargetKind,
    pub name: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: i64,
}

/// One active top-level target membership visible to the requesting actor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentMembership {
    pub target: Target,
    pub role: MembershipRole,
    pub joined_at_ms: i64,
}

/// Authoritative model-facing identity plus optional exact target context.
///
/// A Thread inherits its member roster from `membership_target`, which is its
/// parent Channel or Direct target. For non-Thread targets both targets match.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdentityContext {
    pub agent: AgentProfile,
    pub target: Option<Target>,
    pub membership_target: Option<Target>,
    pub members: Vec<TargetMember>,
}

/// A durable UI invalidation kind. Change rows carry identifiers rather than
/// mutable domain payloads; consumers re-read the authoritative projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    ActorCreated,
    AgentProfileChanged,
    TargetCreated,
    MembershipChanged,
    ThreadFollowChanged,
    MessageCreated,
    TaskCreated,
    TaskUpdated,
    ActivityDoneChanged,
}

impl ChangeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ActorCreated => "actor_created",
            Self::AgentProfileChanged => "agent_profile_changed",
            Self::TargetCreated => "target_created",
            Self::MembershipChanged => "membership_changed",
            Self::ThreadFollowChanged => "thread_follow_changed",
            Self::MessageCreated => "message_created",
            Self::TaskCreated => "task_created",
            Self::TaskUpdated => "task_updated",
            Self::ActivityDoneChanged => "activity_done_changed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "actor_created" => Some(Self::ActorCreated),
            "agent_profile_changed" => Some(Self::AgentProfileChanged),
            "target_created" => Some(Self::TargetCreated),
            "membership_changed" => Some(Self::MembershipChanged),
            "thread_follow_changed" => Some(Self::ThreadFollowChanged),
            "message_created" => Some(Self::MessageCreated),
            "task_created" => Some(Self::TaskCreated),
            "task_updated" => Some(Self::TaskUpdated),
            "activity_done_changed" => Some(Self::ActivityDoneChanged),
            _ => None,
        }
    }
}

/// The title source used by one Activity inbox row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTitleKind {
    Thread,
    Message,
}

#[cfg(feature = "napi")]
impl ActivityTitleKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Message => "message",
        }
    }
}

/// Latest Message preview joined into one Activity inbox row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxReply {
    pub sender_name: String,
    pub sender_kind: ActorKind,
    pub excerpt: String,
    pub at_ms: i64,
}

/// Compact Task state associated with the row's anchor Message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxTask {
    pub number: i64,
    pub status: TaskStatus,
    pub assignee_name: Option<String>,
}

/// One active Channel, Direct, or followed Thread conversation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxItem {
    pub conversation_id: String,
    pub target_kind: TargetKind,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub target_name: String,
    pub title_kind: ActivityTitleKind,
    pub title: String,
    pub latest_reply: Option<ActivityInboxReply>,
    pub last_activity_at_ms: i64,
    pub last_activity_seq: i64,
    pub reply_count: Option<i64>,
    pub task: Option<ActivityInboxTask>,
}

/// One newest-first Activity page plus the total active conversation count.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityInboxPage {
    pub items: Vec<ActivityInboxItem>,
    pub next_cursor: Option<String>,
    pub active_count: i64,
}

/// One monotonically ordered, recipient-snapshotted collaboration change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeEvent {
    pub seq: i64,
    pub kind: ChangeKind,
    pub target_id: Option<String>,
    pub entity_id: String,
    pub created_at_ms: i64,
}

/// A committed immutable text message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    /// Global monotonic database sequence. Expose it as a decimal string over
    /// NAPI so JavaScript never loses integer precision.
    pub seq: i64,
    pub id: String,
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
    pub created_at_ms: i64,
}

/// Authoritative tail page of one target: the exact total message count plus
/// the latest messages in ascending order, read from one consistent snapshot.
/// `count` is always exact, even when it exceeds `messages.len()`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MessageTail {
    pub count: i64,
    pub messages: Vec<Message>,
}

/// One idempotent send command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendMessageRequest {
    pub target_id: String,
    pub author_id: String,
    pub client_request_id: String,
    pub text: String,
}

/// Durable send result plus post-commit runtime effects.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendMessageResult {
    pub message: Message,
    pub recipient_ids: Vec<String>,
    pub wake_agent_ids: Vec<String>,
    pub replayed: bool,
}

/// The current DSH runtime generation bound to a stable Agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeBinding {
    pub agent_id: String,
    pub session_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub preset: String,
    pub bound_at_ms: i64,
}

/// One current runtime whose durable inbox still needs a level-triggered
/// notification. The watermark is the newest authorized, not-yet-model-seen
/// delivery at the time of the scan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingWake {
    pub binding: RuntimeBinding,
    pub pending_seq: i64,
}

/// One message returned by an inbox check.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxMessage {
    pub delivery_id: String,
    pub message: Message,
}

/// A durable record of the exact results returned by one check call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxBatch {
    pub id: Option<String>,
    pub agent_id: String,
    pub session_id: String,
    pub generation: i64,
    pub messages: Vec<InboxMessage>,
    pub contexts: Vec<IdentityContext>,
    pub checked_at_ms: i64,
}

/// Task status remains independent from its optional assignee.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    InReview,
    Done,
}

impl TaskStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::InReview => "in_review",
            Self::Done => "done",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "in_review" => Some(Self::InReview),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

/// Task metadata attached to a top-level Message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Task {
    pub message_id: String,
    pub target_id: String,
    pub number: i64,
    pub status: TaskStatus,
    pub assignee_id: Option<String>,
    pub version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Authoritative snippet of the anchor Message body, resolved in the same
    /// read as the Task row; `None` only when the anchor row is unreadable.
    pub anchor_text: Option<String>,
}

/// One authorization-filtered bootstrap projection and its durable cursor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollabSnapshot {
    pub actor: Actor,
    pub cursor: i64,
    pub targets: Vec<Target>,
    pub followed_thread_ids: Vec<String>,
    pub tasks: Vec<Task>,
}
