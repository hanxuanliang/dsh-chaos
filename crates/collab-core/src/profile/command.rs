use crate::changefeed::{all_actor_ids, insert_change};
use crate::{
    Actor, ActorKind, AgentCharter, AgentProfile, ChangeKind, CollabCore, CollabError, Result,
    new_id, not_found, now_ms, require_non_empty,
};

use super::model::{encode_charter, normalize_charter};
use super::store::ProfileStore;

impl CollabCore {
    /// Create the one local human/user actor.
    pub async fn create_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        self.create_actor(ActorKind::User, handle, display_name, None, None)
            .await
    }

    /// Create a stable Agent actor and record its durable workspace path.
    pub async fn create_agent(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
    ) -> Result<Actor> {
        let charter = AgentCharter::default();
        require_non_empty("workspace_path", workspace_path)?;
        self.create_actor(
            ActorKind::Agent,
            handle,
            display_name,
            Some(workspace_path),
            Some(&charter),
        )
        .await
    }

    /// Create a stable Agent with its first versioned Charter.
    pub async fn create_agent_profile(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
        charter: AgentCharter,
    ) -> Result<AgentProfile> {
        require_non_empty("workspace_path", workspace_path)?;
        let charter = normalize_charter(charter)?;
        let actor = self
            .create_actor(
                ActorKind::Agent,
                handle,
                display_name,
                Some(workspace_path),
                Some(&charter),
            )
            .await?;
        self.agent_profile(&actor.id).await
    }

    /// Replace the mutable display name and Charter under an optimistic Profile fence.
    pub async fn update_agent_profile(
        &self,
        agent_id: &str,
        display_name: &str,
        charter: AgentCharter,
        expected_version: i64,
    ) -> Result<AgentProfile> {
        require_non_empty("agent_id", agent_id)?;
        require_non_empty("display_name", display_name)?;
        if expected_version <= 0 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let display_name = display_name.trim();
        let charter = normalize_charter(charter)?;
        let charter_json = encode_charter(&charter)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ProfileStore::new(connection);
            let current = store.require_profile(agent_id).await?;
            if current.version != expected_version {
                return Err(CollabError::AgentProfileVersionConflict {
                    agent_id: agent_id.to_owned(),
                    expected: expected_version,
                    actual: current.version,
                });
            }
            let next_version = current.version + 1;
            Actor::rename(connection, agent_id, display_name).await?;
            store
                .update_charter(agent_id, &charter_json, next_version, now)
                .await?;
            let actor_ids = all_actor_ids(connection).await?;
            insert_change(
                connection,
                ChangeKind::AgentProfileChanged,
                None,
                agent_id,
                &actor_ids,
                now,
            )
            .await?;
            store.require_profile(agent_id).await
        })
        .await
    }

    /// Delete one Agent's operational state. The actor row and its messages
    /// stay so history never points at a missing author.
    pub async fn delete_agent(&self, actor_id: &str) -> Result<()> {
        require_non_empty("actor_id", actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = crate::actor::find_actor(connection, actor_id).await?;
            if actor.kind != ActorKind::Agent {
                return Err(not_found("agent", actor_id));
            }
            ProfileStore::new(connection)
                .delete_operational_state(actor_id)
                .await?;
            // The actor set changed; reuse actor_created so clients re-pull actors.
            let actor_ids = all_actor_ids(connection).await?;
            insert_change(
                connection,
                ChangeKind::ActorCreated,
                None,
                &actor.id,
                &actor_ids,
                now,
            )
            .await?;
            Ok(())
        })
        .await
    }

    /// Return the stable User for one handle, creating it when absent.
    pub async fn ensure_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        require_non_empty("handle", handle)?;
        require_non_empty("display_name", display_name)?;
        let now = now_ms()?;
        self.write(async |connection| {
            if let Some(actor) = crate::actor::find_actor_by_handle(connection, handle).await? {
                if actor.kind != ActorKind::User {
                    return Err(CollabError::InvalidArgument(format!(
                        "actor handle '{handle}' belongs to an Agent"
                    )));
                }
                if actor.display_name != display_name {
                    // Only leftover default names migrate. A custom display name
                    // is user-owned and must survive later OS-username ensure.
                    if actor.display_name != "Local User" {
                        return Ok(actor);
                    }
                    // Explicit display-name migration on the stable handle: the
                    // actor id, Memberships and Tasks are untouched.
                    Actor::rename(connection, &actor.id, display_name).await?;
                    let actor_ids = all_actor_ids(connection).await?;
                    insert_change(
                        connection,
                        ChangeKind::ActorCreated,
                        None,
                        &actor.id,
                        &actor_ids,
                        now,
                    )
                    .await?;
                    return Ok(Actor {
                        display_name: display_name.to_owned(),
                        ..actor
                    });
                }
                return Ok(actor);
            }

            let actor = Actor {
                id: new_id(),
                kind: ActorKind::User,
                handle: handle.to_owned(),
                display_name: display_name.to_owned(),
                created_at_ms: now,
            };
            actor.insert(connection).await?;
            let actor_ids = all_actor_ids(connection).await?;
            insert_change(
                connection,
                ChangeKind::ActorCreated,
                None,
                &actor.id,
                &actor_ids,
                now,
            )
            .await?;
            Ok(actor)
        })
        .await
    }
}
