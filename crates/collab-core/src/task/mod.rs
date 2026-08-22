//! Task projection, claim ownership, and lifecycle transitions.

mod command;
mod model;
mod query;
pub(crate) mod store;

pub use model::{Task, TaskStatus};

#[cfg(test)]
mod tests;
