//! Transactional local collaboration core for the DSH collab plugin.

/// Reject blank string arguments at an API boundary. The failure names each
/// argument after its own expression; pass `name = value` only when the
/// expression is not the parameter name itself, and `"label" = value` when
/// neither is.
macro_rules! require_non_blank {
    ($($name:ident),+ $(,)?) => {$(
        if $name.trim().is_empty() {
            return Err(crate::CollabError::InvalidArgument(
                concat!(stringify!($name), " must not be blank").into(),
            ));
        }
    )+};
    ($($name:ident = $value:expr),+ $(,)?) => {$(
        if $value.trim().is_empty() {
            return Err(crate::CollabError::InvalidArgument(
                concat!(stringify!($name), " must not be blank").into(),
            ));
        }
    )+};
    ($label:literal = $value:expr $(,)?) => {
        if $value.trim().is_empty() {
            return Err(crate::CollabError::InvalidArgument(
                concat!($label, " must not be blank").into(),
            ));
        }
    };
}

mod activity;
mod actor;
mod changefeed;
mod db;
mod delivery;
mod error;
mod membership;
mod message;
#[cfg(feature = "napi")]
mod napi;
mod profile;
mod runtime;
mod target;
mod task;
#[cfg(test)]
mod tests;
mod thread;

#[cfg(feature = "napi")]
pub use napi::*;

pub use activity::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind,
};
pub use actor::{Actor, ActorKind};
pub use changefeed::{ChangeEvent, ChangeKind, CollabSnapshot};
pub use db::CollabCore;
pub use delivery::{InboxBatch, InboxMessage, PendingWake};
pub use error::{CollabError, Result};
pub use membership::{AgentMembership, IdentityContext, MembershipRole, TargetMember};
pub use message::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub use profile::{AgentCharter, AgentLifecycle, AgentProfile};
pub use runtime::RuntimeBinding;
pub use target::{Target, TargetKind};
pub use task::{Task, TaskStatus};
pub use thread::ThreadSummary;

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

pub(crate) fn new_id() -> String {
    Uuid::now_v7().to_string()
}

pub(crate) fn now_ms() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CollabError::Filesystem(error.to_string()))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| CollabError::Filesystem("system clock is outside i64 milliseconds".into()))
}
