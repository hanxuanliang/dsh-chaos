//! Transactional local collaboration core for the DSH collab plugin.

macro_rules! string_id {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn parse(value: &str) -> Result<Self> {
                if value.trim().is_empty() {
                    return Err(CollabError::InvalidArgument(
                        concat!($label, " must not be blank").into(),
                    ));
                }
                Ok(Self(value.to_owned()))
            }

            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
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
mod model;
#[cfg(feature = "napi")]
mod napi_bridge;
mod profile;
mod runtime;
mod target;
mod task;
#[cfg(test)]
mod test_support;
mod thread;

#[cfg(feature = "napi")]
pub use napi_bridge::*;

pub use activity::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind,
};
pub use actor::{Actor, ActorKind};
pub use db::CollabCore;
pub use error::{CollabError, Result};
pub use membership::{AgentMembership, IdentityContext, MembershipRole, TargetMember};
pub use message::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub use model::{
    ChangeEvent, ChangeKind, CollabSnapshot, InboxBatch, InboxMessage, PendingWake, RuntimeBinding,
};
pub use profile::{AgentCharter, AgentLifecycle, AgentProfile};
pub use target::{Target, TargetKind};
pub use task::{Task, TaskStatus};
pub use thread::ThreadSummary;

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use turso::transaction::TransactionBehavior;
use turso::{Connection, Row};
use uuid::Uuid;

use membership::*;
use message::*;
use runtime::*;
use target::*;
use thread::query::followed_thread_ids_for_actor;

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
