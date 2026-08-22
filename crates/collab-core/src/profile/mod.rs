//! Stable Agent identity, Profile, Charter, and lifecycle operations.

mod command;
mod model;
mod query;
pub(crate) mod store;

pub(crate) use model::encode_charter;

#[cfg(test)]
mod tests;
