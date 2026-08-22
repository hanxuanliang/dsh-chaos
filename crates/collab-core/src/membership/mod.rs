//! Membership mutation, role evidence, and member projections.

mod command;
mod model;
mod query;
mod store;

pub(crate) use model::Membership;
pub use model::{AgentMembership, IdentityContext, MembershipRole, TargetMember};
pub(crate) use store::{
    active_member_ids, identity_context_for, is_active_member, target_change_recipients,
};

#[cfg(test)]
mod tests;
