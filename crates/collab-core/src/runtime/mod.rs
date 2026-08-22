//! Stable Agent to DSH Session generation bindings.

mod command;
mod model;
mod query;
mod store;

pub use model::RuntimeBinding;
pub(crate) use store::require_current_binding;
