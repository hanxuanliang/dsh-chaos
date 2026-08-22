use crate::TaskStatus;

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
