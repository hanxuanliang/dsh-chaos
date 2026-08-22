//! Target identity, authorization routes, and Channel/Direct creation.

mod command;
mod model;
mod store;

pub use model::{Target, TargetKind};
pub(crate) use model::{TargetRoute, parse_target_kind, require_target, require_target_access};
pub(crate) use store::{find_target, targets_for_actor};
