//! Stable Agent identity, Profile, Charter, and lifecycle operations.

mod command;
mod model;
mod query;
pub(crate) mod store;

pub(crate) use model::encode_charter;
pub use model::{AgentCharter, AgentLifecycle, AgentProfile};

#[cfg(test)]
mod tests;
