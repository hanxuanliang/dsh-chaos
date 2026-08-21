use thiserror::Error;

/// Stable failure vocabulary exposed by the collab core.
#[derive(Debug, Error)]
pub enum CollabError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("core is closed")]
    Closed,
    #[error("{entity} '{id}' was not found")]
    NotFound { entity: &'static str, id: String },
    #[error("actor '{actor_id}' cannot {action} target '{target_id}'")]
    PermissionDenied {
        actor_id: String,
        action: &'static str,
        target_id: String,
    },
    #[error("task '{message_id}' is already claimed by '{assignee_id}'")]
    TaskAlreadyClaimed {
        message_id: String,
        assignee_id: String,
    },
    #[error("task '{message_id}' cannot transition from '{status}'")]
    TaskTransitionDenied { message_id: String, status: String },
    #[error(
        "task '{message_id}' version conflict: expected '{expected}', current version is '{actual}'"
    )]
    TaskVersionConflict {
        message_id: String,
        expected: i64,
        actual: i64,
    },
    #[error(
        "agent profile '{agent_id}' version conflict: expected '{expected}', current version is '{actual}'"
    )]
    AgentProfileVersionConflict {
        agent_id: String,
        expected: i64,
        actual: i64,
    },
    #[error(
        "change cursor '{after_seq}' is outside retained range '{minimum_cursor}'..='{maximum_cursor}'"
    )]
    ChangeCursorOutOfRange {
        after_seq: i64,
        minimum_cursor: i64,
        maximum_cursor: i64,
    },
    #[error("runtime generation mismatch for agent '{agent_id}'")]
    RuntimeGenerationMismatch { agent_id: String },
    #[error("schema version '{found}' is not supported; expected '{expected}'")]
    SchemaVersionMismatch { found: String, expected: u32 },
    #[error("database error: {0}")]
    Database(String),
    #[error("filesystem error: {0}")]
    Filesystem(String),
    #[cfg(test)]
    #[error("injected send failure")]
    InjectedSendFailure,
}

impl CollabError {
    /// Machine-oriented code that remains stable across wording changes.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidArgument(_) => "invalid_argument",
            Self::Closed => "closed",
            Self::NotFound { .. } => "not_found",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::TaskAlreadyClaimed { .. } => "task_already_claimed",
            Self::TaskTransitionDenied { .. } => "task_transition_denied",
            Self::TaskVersionConflict { .. } => "task_version_conflict",
            Self::AgentProfileVersionConflict { .. } => "agent_profile_version_conflict",
            Self::ChangeCursorOutOfRange { .. } => "change_cursor_resync_required",
            Self::RuntimeGenerationMismatch { .. } => "runtime_generation_mismatch",
            Self::SchemaVersionMismatch { .. } => "schema_version_mismatch",
            Self::Database(_) => "database_error",
            Self::Filesystem(_) => "filesystem_error",
            #[cfg(test)]
            Self::InjectedSendFailure => "injected_send_failure",
        }
    }
}

impl From<turso::Error> for CollabError {
    fn from(error: turso::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<std::io::Error> for CollabError {
    fn from(error: std::io::Error) -> Self {
        Self::Filesystem(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CollabError>;
