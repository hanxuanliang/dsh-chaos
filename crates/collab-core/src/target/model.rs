use serde::{Deserialize, Serialize};
use turso::Connection;

use crate::actor::{Actor, ActorId};
use crate::membership::Membership;
use crate::{CollabError, Result};

/// A collab target kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Channel,
    Direct,
    Thread,
}

impl TargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Direct => "direct",
            Self::Thread => "thread",
        }
    }
}

/// A stable exact collab target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Target {
    pub id: String,
    pub kind: TargetKind,
    pub name: String,
    pub parent_target_id: Option<String>,
    pub root_message_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: i64,
}

/// One active target's authorization route: kind proof plus the Thread
/// inheritance parent when present.
#[derive(Clone, Debug)]
pub(crate) struct TargetRoute {
    pub(crate) kind: TargetKind,
    pub(crate) parent_target_id: Option<String>,
}

impl TargetRoute {
    /// Load one active target's authorization route, failing when absent.
    /// Existence and topology proof: a Thread must carry a non-Thread parent.
    pub(crate) async fn require(connection: &Connection, target_id: &str) -> Result<Self> {
        let mut rows = connection
            .query(
                "SELECT kind, parent_target_id
                 FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
                [target_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "active target",
                id: target_id.to_owned(),
            });
        };
        let kind_text = row.get::<String>(0)?;
        let parent_target_id = row.get::<Option<String>>(1)?;
        drop(rows);
        let kind = parse_target_kind(target_id, &kind_text)?;
        if kind == TargetKind::Thread && parent_target_id.is_none() {
            return Err(CollabError::Database(format!(
                "Thread target '{target_id}' has no parent target"
            )));
        }
        if kind != TargetKind::Thread && parent_target_id.is_some() {
            return Err(CollabError::Database(format!(
                "non-Thread target '{target_id}' unexpectedly has a parent"
            )));
        }
        if let Some(parent_target_id) = parent_target_id.as_deref() {
            let mut parent_rows = connection
                .query(
                    "SELECT kind FROM targets
                     WHERE id = ?1 AND archived_at_ms IS NULL",
                    [parent_target_id],
                )
                .await?;
            let Some(parent_row) = parent_rows.next().await? else {
                return Err(CollabError::NotFound {
                    entity: "active Thread parent target",
                    id: parent_target_id.to_owned(),
                });
            };
            let parent_kind_text = parent_row.get::<String>(0)?;
            let parent_kind = parse_target_kind(parent_target_id, &parent_kind_text)?;
            if parent_kind == TargetKind::Thread {
                return Err(CollabError::Database(format!(
                    "Thread target '{target_id}' has a Thread parent"
                )));
            }
        }
        Ok(Self {
            kind,
            parent_target_id,
        })
    }

    pub(crate) fn permission_target_id<'a>(&'a self, exact_target_id: &'a str) -> &'a str {
        self.parent_target_id.as_deref().unwrap_or(exact_target_id)
    }
}

pub(crate) fn parse_target_kind(target_id: &str, value: &str) -> Result<TargetKind> {
    match value {
        "channel" => Ok(TargetKind::Channel),
        "direct" => Ok(TargetKind::Direct),
        "thread" => Ok(TargetKind::Thread),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' has unknown kind '{other}'"
        ))),
    }
}

pub(crate) async fn require_target(connection: &Connection, target_id: &str) -> Result<TargetKind> {
    Ok(TargetRoute::require(connection, target_id).await?.kind)
}

/// Certify that `actor_id` exists and is an active member of `target_id`'s
/// permission target, returning the resolved route.
pub(crate) async fn require_target_access(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<TargetRoute> {
    let route = TargetRoute::require(connection, target_id).await?;
    let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
    Membership::require(connection, route.permission_target_id(target_id), &actor).await?;
    Ok(route)
}
