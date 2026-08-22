use std::collections::BTreeSet;

use crate::{AgentCharter, AgentLifecycle, CollabError, Result};

pub(crate) const CHARTER_SUMMARY_MAX: usize = 4_000;
pub(crate) const CHARTER_LIST_MAX: usize = 32;
pub(crate) const CHARTER_CAPABILITY_MAX: usize = 80;
pub(crate) const CHARTER_CONSTRAINT_MAX: usize = 500;

pub(crate) fn normalize_charter(mut charter: AgentCharter) -> Result<AgentCharter> {
    if charter.schema_version != 1 {
        return Err(CollabError::InvalidArgument(format!(
            "unsupported Charter schema version '{}'",
            charter.schema_version
        )));
    }
    charter.summary = charter.summary.trim().to_owned();
    if charter.summary.chars().count() > CHARTER_SUMMARY_MAX {
        return Err(CollabError::InvalidArgument(format!(
            "Charter summary exceeds {CHARTER_SUMMARY_MAX} characters"
        )));
    }
    charter.capabilities =
        normalize_charter_list("capabilities", charter.capabilities, CHARTER_CAPABILITY_MAX)?;
    charter.constraints =
        normalize_charter_list("constraints", charter.constraints, CHARTER_CONSTRAINT_MAX)?;
    Ok(charter)
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

pub(crate) fn encode_charter(charter: &AgentCharter) -> Result<String> {
    serde_json::to_string(charter)
        .map_err(|error| CollabError::Database(format!("encode Agent Charter: {error}")))
}

pub(crate) fn decode_charter(agent_id: &str, value: &str) -> Result<AgentCharter> {
    let charter: AgentCharter = serde_json::from_str(value).map_err(|error| {
        CollabError::Database(format!("Agent '{agent_id}' Charter is malformed: {error}"))
    })?;
    normalize_charter(charter).map_err(|error| {
        CollabError::Database(format!("Agent '{agent_id}' Charter is invalid: {error}"))
    })
}

pub(crate) fn parse_agent_lifecycle(agent_id: &str, value: &str) -> Result<AgentLifecycle> {
    match value {
        "active" => Ok(AgentLifecycle::Active),
        "archived" => Ok(AgentLifecycle::Archived),
        other => Err(CollabError::Database(format!(
            "Agent '{agent_id}' has unknown lifecycle '{other}'"
        ))),
    }
}
