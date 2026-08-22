//! Transactional local collaboration core for the DSH collab plugin.

mod activity;
mod actor;
mod changefeed;
mod core;
mod db;
mod delivery;
mod error;
mod ids;
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
pub use core::CollabCore;
pub use error::{CollabError, Result};
pub use message::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub use model::{
    Actor, ActorKind, AgentMembership, ChangeEvent, ChangeKind, CollabSnapshot, IdentityContext,
    InboxBatch, InboxMessage, MembershipRole, PendingWake, RuntimeBinding, Target, TargetKind,
    TargetMember,
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
use profile::*;
use runtime::*;
use target::*;
use thread::query::followed_thread_ids_for_actor;
