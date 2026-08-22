//! Task projection, claim ownership, and lifecycle transitions.

mod command;
mod model;
mod query;
pub(crate) mod store;

#[cfg(test)]
mod tests;
