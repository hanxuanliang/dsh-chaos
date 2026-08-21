//! Transactional Actor creation shared by User and Agent Profile entry points.

use super::*;
use crate::db::{FromRow, QueryRows};
use crate::ids::ActorId;

impl FromRow for Actor {
    fn from_row(row: &Row) -> Result<Self> {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        Ok(Self {
            kind: parse_actor_kind(&id, &kind_text)?,
            id,
            handle: row.get(2)?,
            display_name: row.get(3)?,
            created_at_ms: row.get(4)?,
        })
    }
}

impl Actor {
    /// Load one Actor by id, failing when absent. Presence proof for
    /// authorization chains.
    pub(crate) async fn require(connection: &Connection, id: &ActorId) -> Result<Self> {
        connection
            .query_row::<Self>(
                "SELECT id, kind, handle, display_name, created_at_ms
                 FROM actors WHERE id = ?1",
                [id.as_str()],
            )
            .await?
            .ok_or_else(|| not_found("actor", id.as_str()))
    }
}

impl CollabCore {
    pub(super) async fn create_actor(
        &self,
        kind: ActorKind,
        handle: &str,
        display_name: &str,
        workspace_path: Option<&str>,
        charter: Option<&AgentCharter>,
    ) -> Result<Actor> {
        self.assert_open()?;
        require_non_empty("handle", handle)?;
        require_non_empty("display_name", display_name)?;
        let now = now_ms()?;
        let actor = Actor {
            id: new_id(),
            kind,
            handle: handle.to_owned(),
            display_name: display_name.to_owned(),
            created_at_ms: now,
        };
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    actor.id.as_str(),
                    actor.kind.as_str(),
                    actor.handle.as_str(),
                    actor.display_name.as_str(),
                    actor.created_at_ms,
                ),
            )
            .await?;
        if let Some(workspace_path) = workspace_path {
            let resolved_path = workspace_path.replace("{id}", actor.id.as_str());
            let charter = charter.ok_or_else(|| {
                CollabError::InvalidArgument("Agent creation requires a Charter".into())
            })?;
            let charter_json = encode_charter(charter)?;
            transaction
                .execute(
                    "INSERT INTO agents
                     (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms,
                      charter_json, profile_version)
                     VALUES (?1, ?2, 'active', ?3, ?3, ?4, 1)",
                    (
                        actor.id.as_str(),
                        resolved_path.as_str(),
                        now,
                        charter_json.as_str(),
                    ),
                )
                .await?;
        } else if charter.is_some() {
            return Err(CollabError::InvalidArgument(
                "User creation cannot carry an Agent Charter".into(),
            ));
        }
        let actor_ids = all_actor_ids(&transaction).await?;
        insert_change(
            &transaction,
            ChangeKind::ActorCreated,
            None,
            &actor.id,
            &actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(actor)
    }
}

pub(crate) fn parse_actor_kind(actor_id: &str, value: &str) -> Result<ActorKind> {
    match value {
        "user" => Ok(ActorKind::User),
        "agent" => Ok(ActorKind::Agent),
        other => Err(CollabError::Database(format!(
            "actor '{actor_id}' has unknown kind '{other}'"
        ))),
    }
}

pub(crate) async fn find_actor(connection: &Connection, actor_id: &str) -> Result<Actor> {
    Actor::require(connection, &ActorId::parse(actor_id)?).await
}

pub(crate) async fn find_actor_by_handle(
    connection: &Connection,
    handle: &str,
) -> Result<Option<Actor>> {
    connection
        .query_row::<Actor>(
            "SELECT id, kind, handle, display_name, created_at_ms
             FROM actors WHERE handle = ?1",
            [handle],
        )
        .await
}

pub(crate) async fn require_actor(connection: &Connection, actor_id: &str) -> Result<ActorKind> {
    let mut rows = connection
        .query("SELECT kind FROM actors WHERE id = ?1", [actor_id])
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("actor", actor_id));
    };
    parse_actor_kind(actor_id, &row.get::<String>(0)?)
}

pub(crate) async fn require_agent(connection: &Connection, agent_id: &str) -> Result<()> {
    if require_actor(connection, agent_id).await? != ActorKind::Agent {
        return Err(not_found("agent", agent_id));
    }
    let mut rows = connection
        .query(
            "SELECT 1 FROM agents WHERE actor_id = ?1 AND lifecycle = 'active'",
            [agent_id],
        )
        .await?;
    if rows.next().await?.is_none() {
        return Err(not_found("active agent", agent_id));
    }
    Ok(())
}
