//! Stable Agent identity, Profile, Charter, and lifecycle operations.

mod model;
pub(crate) mod store;

pub use model::{AgentCharter, AgentLifecycle, AgentProfile};

use crate::actor::{ActorId, insert_actor};
use crate::changefeed::{all_actor_ids, insert_change};
use crate::membership::identity_context_for;
use crate::{
    Actor, ActorKind, ChangeKind, CollabCore, CollabError, IdentityContext, Result, now_ms,
};

use model::{encode_charter, normalize_charter};
use store::ProfileStore;

impl CollabCore {
    /// Create the one local human/user actor.
    pub async fn create_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        let now = now_ms()?;
        self.write(async |connection| {
            insert_actor(connection, ActorKind::User, handle, display_name, now).await
        })
        .await
    }

    /// Create a stable Agent actor and record its durable workspace path.
    pub async fn create_agent(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
    ) -> Result<Actor> {
        self.create_agent_actor(
            handle,
            display_name,
            workspace_path,
            &AgentCharter::default(),
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
        let charter = normalize_charter(charter)?;
        let actor = self
            .create_agent_actor(handle, display_name, workspace_path, &charter)
            .await?;
        self.agent_profile(&actor.id).await
    }

    /// Shared Agent creation: one Actor, its agents row, and the ActorCreated
    /// change, all committed in one transaction.
    async fn create_agent_actor(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
        charter: &AgentCharter,
    ) -> Result<Actor> {
        require_non_blank!(workspace_path);
        let charter_json = encode_charter(charter)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let actor =
                insert_actor(connection, ActorKind::Agent, handle, display_name, now).await?;
            let workspace_path = workspace_path.replace("{id}", actor.id.as_str());
            ProfileStore::new(connection)
                .insert_agent(&actor.id, &workspace_path, &charter_json, now)
                .await?;
            Ok(actor)
        })
        .await
    }

    /// Replace the mutable display name and Charter under an optimistic Profile fence.
    pub async fn update_agent_profile(
        &self,
        agent_id: &str,
        display_name: &str,
        charter: AgentCharter,
        expected_version: i64,
    ) -> Result<AgentProfile> {
        require_non_blank!(agent_id, display_name);
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
        require_non_blank!(actor_id);
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if actor.kind != ActorKind::Agent {
                return Err(CollabError::NotFound {
                    entity: "agent",
                    id: actor_id.to_owned(),
                });
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
        require_non_blank!(handle, display_name);
        let now = now_ms()?;
        self.write(async |connection| {
            if let Some(actor) = Actor::find_by_handle(connection, handle).await? {
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

            insert_actor(connection, ActorKind::User, handle, display_name, now).await
        })
        .await
    }
}

impl CollabCore {
    /// Read one active stable Agent Profile independently from its DSH Session.
    pub async fn agent_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        require_non_blank!(agent_id);
        self.read(async |connection| {
            ProfileStore::new(connection)
                .require_profile(agent_id)
                .await
        })
        .await
    }

    /// List every live Agent Profile in one coherent directory read.
    pub async fn list_agent_profiles(&self, actor_id: &str) -> Result<Vec<AgentProfile>> {
        require_non_blank!(actor_id);
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            ProfileStore::new(connection).directory().await
        })
        .await
    }

    /// Return the caller's stable identity and optional exact target roster.
    pub async fn identity_context(
        &self,
        agent_id: &str,
        target_id: Option<&str>,
    ) -> Result<IdentityContext> {
        require_non_blank!(agent_id);
        self.read(async |connection| identity_context_for(connection, agent_id, target_id).await)
            .await
    }
}
