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

pub use core::CollabCore;
pub use error::{CollabError, Result};
pub use model::{
    ActivityInboxItem, ActivityInboxPage, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind,
    Actor, ActorKind, AgentCharter, AgentLifecycle, AgentMembership, AgentProfile, ChangeEvent,
    ChangeKind, CollabSnapshot, IdentityContext, InboxBatch, InboxMessage, MembershipRole, Message,
    MessageTail, PendingWake, RuntimeBinding, SendMessageRequest, SendMessageResult, Target,
    TargetKind, TargetMember, Task, TaskStatus,
};
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
use task::*;
use thread::query::followed_thread_ids_for_actor;
