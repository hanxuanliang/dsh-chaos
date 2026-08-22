//! Stable actor identity and kind, with persistence anchored on the domain type.

mod command;
mod model;

pub(crate) use command::insert_actor;
pub use model::{Actor, ActorKind};
pub(crate) use model::{ActorId, parse_actor_kind};
