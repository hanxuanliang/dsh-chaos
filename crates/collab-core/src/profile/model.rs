use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{Actor, CollabError, Result};

/// Versioned, stable collaboration responsibilities for one Agent.
///
/// The Rust type is the contract; storage uses canonical JSON so future schema
/// versions can add bounded fields without turning the charter into a free-form
/// key/value bag.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCharter {
    pub schema_version: u32,
    pub summary: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
}

impl Default for AgentCharter {
    fn default() -> Self {
        Self {
            schema_version: 1,
            summary: String::new(),
            capabilities: Vec::new(),
            constraints: Vec::new(),
        }
    }
}

/// Operational lifecycle of a stable Agent Profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Active,
    Archived,
}

impl AgentLifecycle {
    /// Decode one row's lifecycle text, rejecting unknown values.
    pub(crate) fn parse(agent_id: &str, value: &str) -> Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            other => Err(CollabError::Database(format!(
                "Agent '{agent_id}' has unknown lifecycle '{other}'"
            ))),
        }
    }

    #[cfg(feature = "napi")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

/// Stable Agent identity and workspace state, independent from its DSH Session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentProfile {
    pub actor: Actor,
    pub workspace_path: String,
    pub lifecycle: AgentLifecycle,
    pub charter: AgentCharter,
    pub version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub(crate) const CHARTER_SUMMARY_MAX: usize = 4_000;
pub(crate) const CHARTER_LIST_MAX: usize = 32;
pub(crate) const CHARTER_CAPABILITY_MAX: usize = 80;
pub(crate) const CHARTER_CONSTRAINT_MAX: usize = 500;

impl AgentCharter {
    /// Trim fields and enforce the Charter size contract.
    pub(crate) fn normalize(mut self) -> Result<Self> {
        if self.schema_version != 1 {
            return Err(CollabError::InvalidArgument(format!(
                "unsupported Charter schema version '{}'",
                self.schema_version
            )));
        }
        self.summary = self.summary.trim().to_owned();
        if self.summary.chars().count() > CHARTER_SUMMARY_MAX {
            return Err(CollabError::InvalidArgument(format!(
                "Charter summary exceeds {CHARTER_SUMMARY_MAX} characters"
            )));
        }
        self.capabilities =
            normalize_charter_list("capabilities", self.capabilities, CHARTER_CAPABILITY_MAX)?;
        self.constraints =
            normalize_charter_list("constraints", self.constraints, CHARTER_CONSTRAINT_MAX)?;
        Ok(self)
    }

    /// Serialize for storage.
    pub(crate) fn encode(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|error| CollabError::Database(format!("encode Agent Charter: {error}")))
    }

    /// Decode stored JSON back into a normalized Charter.
    pub(crate) fn decode(agent_id: &str, value: &str) -> Result<Self> {
        let charter: Self = serde_json::from_str(value).map_err(|error| {
            CollabError::Database(format!("Agent '{agent_id}' Charter is malformed: {error}"))
        })?;
        charter.normalize().map_err(|error| {
            CollabError::Database(format!("Agent '{agent_id}' Charter is invalid: {error}"))
        })
    }
}

fn normalize_charter_list(name: &str, values: Vec<String>, item_max: usize) -> Result<Vec<String>> {
    if values.len() > CHARTER_LIST_MAX {
        return Err(CollabError::InvalidArgument(format!(
            "Charter {name} exceed {CHARTER_LIST_MAX} entries"
        )));
    }
    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            return Err(CollabError::InvalidArgument(format!(
                "Charter {name} cannot contain blank entries"
            )));
        }
        if value.chars().count() > item_max {
            return Err(CollabError::InvalidArgument(format!(
                "Charter {name} entry exceeds {item_max} characters"
            )));
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            normalized.push(value.to_owned());
        }
    }
    Ok(normalized)
}
