//! Thread creation, attention subscriptions, and read projections.

mod command;
mod model;
pub(crate) mod query;
pub(crate) mod store;

pub use model::ThreadSummary;

#[cfg(test)]
mod tests;
