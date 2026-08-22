//! Immutable Message writes, recipient snapshots, and authorized history reads.

mod command;
mod model;
mod query;
pub(crate) mod store;

pub use model::{Message, MessageTail, SendMessageRequest, SendMessageResult};
pub(crate) use model::{StoredTextBody, stored_text};

#[cfg(test)]
mod tests;
