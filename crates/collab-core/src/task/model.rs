use serde::{Deserialize, Serialize};

use crate::membership::MembershipRole;
use crate::target::AccessGrant;
use crate::{CollabError, Result};

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
    /// The explicit lifecycle: only these edges may be committed.
    pub(crate) const fn can_transition_to(self, to: TaskStatus) -> bool {
        matches!(
            (self, to),
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

/// One committed decision over a loaded Task: the next row state, the audit
/// event, and whether a change notification must be published. Commands
/// produce a `TaskTransition` through pure decisions; stores persist it.
pub(crate) struct TaskTransition {
    pub message_id: String,
    pub target_id: String,
    pub actor_id: String,
    pub kind: TaskEventKind,
    pub next_status: Option<TaskStatus>,
    pub next_assignee_id: Option<Option<String>>,
    pub next_version: i64,
    pub publish: bool,
    pub task_after: Task,
}

impl TaskTransition {
    fn evolve(
        current: &Task,
        actor_id: &str,
        kind: TaskEventKind,
        next_status: Option<TaskStatus>,
        next_assignee_id: Option<Option<String>>,
        publish: bool,
        now: i64,
    ) -> Self {
        let next_version = current.version + 1;
        let mut task_after = current.clone();
        task_after.version = next_version;
        task_after.updated_at_ms = now;
        if let Some(status) = next_status {
            task_after.status = status;
        }
        if let Some(assignee_id) = next_assignee_id.clone() {
            task_after.assignee_id = assignee_id;
        }
        Self {
            message_id: current.message_id.clone(),
            target_id: current.target_id.clone(),
            actor_id: actor_id.to_owned(),
            kind,
            next_status,
            next_assignee_id,
            next_version,
            publish,
            task_after,
        }
    }

    pub(crate) fn event(&self, now: i64) -> TaskEvent<'_> {
        TaskEvent {
            message_id: &self.message_id,
            actor_id: &self.actor_id,
            kind: self.kind,
            from_status: None,
            to_status: self.next_status,
            from_assignee_id: None,
            to_assignee_id: self.next_assignee_id.as_ref().and_then(|a| a.as_deref()),
            task_version: self.next_version,
            created_at_ms: now,
        }
    }
}

/// The claim decision: one of refusal, idempotent replay, or a transition.
pub(crate) enum TaskDecision {
    Idempotent,
    Transition(Box<TaskTransition>),
}

/// A loaded Task plus the access evidence for the acting actor. Commands
/// decide through this value; every guard sees the same proven facts.
pub(crate) struct LoadedTask {
    pub task: Task,
    pub grant: AccessGrant,
}

impl LoadedTask {
    pub(crate) fn not_found(message_id: &str) -> CollabError {
        CollabError::NotFound {
            entity: "task",
            id: message_id.to_owned(),
        }
    }

    /// Claim an unowned Task. A second actor receives a typed concurrency
    /// conflict; the current assignee repeats the claim idempotently.
    pub(crate) fn claim(self, now: i64) -> Result<TaskDecision> {
        let actor_id = self.grant.actor.id.clone();
        if self.task.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: self.task.message_id.clone(),
                status: self.task.status.as_str().to_owned(),
            });
        }
        if let Some(assignee_id) = &self.task.assignee_id {
            if assignee_id == &actor_id {
                return Ok(TaskDecision::Idempotent);
            }
            return Err(CollabError::TaskAlreadyClaimed {
                message_id: self.task.message_id.clone(),
                assignee_id: assignee_id.clone(),
            });
        }
        let transition = TaskTransition::evolve(
            &self.task,
            actor_id.as_str(),
            TaskEventKind::Claimed,
            Some(TaskStatus::InProgress),
            Some(Some(actor_id.clone())),
            true,
            now,
        );
        Ok(TaskDecision::Transition(Box::new(transition)))
    }

    /// Release this actor's Task, preserving its independent status.
    pub(crate) fn unclaim(self, expected_version: i64, now: i64) -> Result<TaskDecision> {
        let actor_id = self.grant.actor.id.clone();
        if self.task.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: self.task.message_id.clone(),
                expected: expected_version,
                actual: self.task.version,
            });
        }
        let Some(assignee_id) = self.task.assignee_id.as_deref() else {
            return Ok(TaskDecision::Idempotent);
        };
        if assignee_id != actor_id {
            return Err(CollabError::PermissionDenied {
                actor_id,
                action: "unclaim another actor's task in",
                target_id: self.task.target_id.clone(),
            });
        }
        if self.task.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: self.task.message_id.clone(),
                status: self.task.status.as_str().to_owned(),
            });
        }
        let transition = TaskTransition::evolve(
            &self.task,
            actor_id.as_str(),
            TaskEventKind::Unclaimed,
            None,
            Some(None),
            true,
            now,
        );
        Ok(TaskDecision::Transition(Box::new(transition)))
    }

    /// Move the Task through the explicit lifecycle. Only the owner may
    /// transition another actor's claimed Task.
    pub(crate) fn change_status(
        self,
        status: TaskStatus,
        expected_version: i64,
        now: i64,
    ) -> Result<TaskDecision> {
        let actor_id = self.grant.actor.id.clone();
        if self.task.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: self.task.message_id.clone(),
                expected: expected_version,
                actual: self.task.version,
            });
        }
        if self
            .task
            .assignee_id
            .as_deref()
            .is_some_and(|assignee_id| assignee_id != actor_id)
            && !matches!(self.grant.role, MembershipRole::Owner)
        {
            return Err(CollabError::PermissionDenied {
                actor_id,
                action: "update another actor's task in",
                target_id: self.task.target_id.clone(),
            });
        }
        if self.task.status == status {
            return Ok(TaskDecision::Idempotent);
        }
        if !self.task.status.can_transition_to(status) {
            return Err(CollabError::TaskTransitionDenied {
                message_id: self.task.message_id.clone(),
                status: self.task.status.as_str().to_owned(),
            });
        }
        let transition = TaskTransition::evolve(
            &self.task,
            actor_id.as_str(),
            TaskEventKind::StatusChanged,
            Some(status),
            None,
            true,
            now,
        );
        Ok(TaskDecision::Transition(Box::new(transition)))
    }
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
