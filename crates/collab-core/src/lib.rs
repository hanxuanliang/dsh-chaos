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
mod core;
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
pub use core::CollabCore;
pub use error::{CollabError, Result};
pub use message::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub use model::{
    AgentMembership, ChangeEvent, ChangeKind, CollabSnapshot, IdentityContext, InboxBatch,
    InboxMessage, MembershipRole, PendingWake, RuntimeBinding, Target, TargetKind, TargetMember,
};
pub use profile::{AgentCharter, AgentLifecycle, AgentProfile};
pub use task::{Task, TaskStatus};
pub use thread::ThreadSummary;

use std::collections::BTreeSet;

use turso::transaction::TransactionBehavior;
use turso::{Connection, Row};

use actor::*;
use changefeed::*;
use core::*;
use membership::*;
use message::*;
use runtime::*;
use target::*;
use thread::query::followed_thread_ids_for_actor;
