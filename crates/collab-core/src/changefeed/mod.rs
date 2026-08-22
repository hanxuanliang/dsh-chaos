//! Durable, recipient-filtered change notifications for client synchronization.

mod command;
mod model;
mod query;
mod store;

pub use model::{ChangeEvent, ChangeKind, CollabSnapshot};
pub(crate) use store::{all_actor_ids, insert_change, insert_target_change};

#[cfg(test)]
mod tests;
