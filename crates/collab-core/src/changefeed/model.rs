use serde::{Deserialize, Serialize};
use turso::Row;

use crate::db::FromRow;
use crate::{Actor, CollabError, Result, Target, Task};

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

impl FromRow for ChangeEvent {
    fn from_row(row: &Row) -> Result<Self> {
        let seq = row.get::<i64>(0)?;
        let kind_text = row.get::<String>(1)?;
        let Some(kind) = ChangeKind::parse(&kind_text) else {
            return Err(CollabError::Database(format!(
                "change '{seq}' has unknown kind '{kind_text}'"
            )));
        };
        Ok(Self {
            seq,
            kind,
            target_id: row.get(2)?,
            entity_id: row.get(3)?,
            created_at_ms: row.get(4)?,
        })
    }
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
