//! Thread creation, attention subscriptions, and read projections.

mod command;
mod model;
pub(crate) mod query;
pub(crate) mod store;

pub(crate) use model::ThreadId;
pub use model::ThreadSummary;
