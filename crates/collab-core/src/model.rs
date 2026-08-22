use serde::{Deserialize, Serialize};

use crate::{Actor, IdentityContext, Message, RuntimeBinding, Target, Task};

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

/// One monotonically ordered, recipient-snapshotted collaboration change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeEvent {
    pub seq: i64,
    pub kind: ChangeKind,
    pub target_id: Option<String>,
    pub entity_id: String,
    pub created_at_ms: i64,
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

/// One authorization-filtered bootstrap projection and its durable cursor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollabSnapshot {
    pub actor: Actor,
    pub cursor: i64,
    pub targets: Vec<Target>,
    pub followed_thread_ids: Vec<String>,
    pub tasks: Vec<Task>,
}
