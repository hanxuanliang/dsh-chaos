use serde::{Deserialize, Serialize};

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

/// The explicit Task lifecycle: only these edges may be committed.
pub(crate) fn task_transition_allowed(from: TaskStatus, to: TaskStatus) -> bool {
    matches!(
        (from, to),
        (TaskStatus::Todo, TaskStatus::InProgress)
            | (
                TaskStatus::InProgress,
                TaskStatus::Todo | TaskStatus::InReview
            )
            | (
                TaskStatus::InReview,
                TaskStatus::InProgress | TaskStatus::Done
            )
            | (TaskStatus::Done, TaskStatus::InProgress)
    )
}

pub(crate) struct NewTask<'message> {
    pub(crate) message_id: &'message str,
    pub(crate) target_id: &'message str,
    pub(crate) number: i64,
    pub(crate) created_at_ms: i64,
}

/// One lifecycle imprint on the task_events audit trail.
#[derive(Clone, Copy)]
pub(crate) enum TaskEventKind {
    Created,
    Claimed,
    Unclaimed,
    StatusChanged,
}

impl TaskEventKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Claimed => "claimed",
            Self::Unclaimed => "unclaimed",
            Self::StatusChanged => "status_changed",
        }
    }
}

pub(crate) struct TaskEvent<'message> {
    pub(crate) message_id: &'message str,
    pub(crate) actor_id: &'message str,
    pub(crate) kind: TaskEventKind,
    pub(crate) from_status: Option<TaskStatus>,
    pub(crate) to_status: Option<TaskStatus>,
    pub(crate) from_assignee_id: Option<&'message str>,
    pub(crate) to_assignee_id: Option<&'message str>,
    pub(crate) task_version: i64,
    pub(crate) created_at_ms: i64,
}
