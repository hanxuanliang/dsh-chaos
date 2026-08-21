//! Stable Agent identity, Profile, Charter, and lifecycle operations.

use super::*;

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

    /// Read one active stable Agent Profile independently from its DSH Session.
    pub async fn agent_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        let connection = self.connection.lock().await;
        find_agent_profile(&connection, agent_id).await
    }

    /// List every live Agent Profile in one coherent directory read.
    pub async fn list_agent_profiles(&self, actor_id: &str) -> Result<Vec<AgentProfile>> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        agent_profiles(&connection).await
    }

    /// Replace the mutable display name and Charter under an optimistic Profile fence.
    pub async fn update_agent_profile(
        &self,
        agent_id: &str,
        display_name: &str,
        charter: AgentCharter,
        expected_version: i64,
    ) -> Result<AgentProfile> {
        self.assert_open()?;
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
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let current = find_agent_profile(&transaction, agent_id).await?;
        if current.version != expected_version {
            return Err(CollabError::AgentProfileVersionConflict {
                agent_id: agent_id.to_owned(),
                expected: expected_version,
                actual: current.version,
            });
        }
        let next_version = current.version + 1;
        transaction
            .execute(
                "UPDATE actors SET display_name = ?2 WHERE id = ?1",
                (agent_id, display_name),
            )
            .await?;
        transaction
            .execute(
                "UPDATE agents
                 SET charter_json = ?2, profile_version = ?3, updated_at_ms = ?4
                 WHERE actor_id = ?1",
                (agent_id, charter_json.as_str(), next_version, now),
            )
            .await?;
        let actor_ids = all_actor_ids(&transaction).await?;
        insert_change(
            &transaction,
            ChangeKind::AgentProfileChanged,
            None,
            agent_id,
            &actor_ids,
            now,
        )
        .await?;
        let profile = find_agent_profile(&transaction, agent_id).await?;
        transaction.commit().await?;
        Ok(profile)
    }

    /// Return the caller's stable identity and optional exact target roster.
    pub async fn identity_context(
        &self,
        agent_id: &str,
        target_id: Option<&str>,
    ) -> Result<IdentityContext> {
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        let connection = self.connection.lock().await;
        identity_context_for(&connection, agent_id, target_id).await
    }

    /// Delete one Agent's operational state. The actor row and its messages
    /// stay so history never points at a missing author.
    pub async fn delete_agent(&self, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let actor = find_actor(&transaction, actor_id).await?;
        if actor.kind != ActorKind::Agent {
            return Err(not_found("agent", actor_id));
        }
        transaction
            .execute("DELETE FROM memberships WHERE actor_id = ?1", [actor_id])
            .await?;
        transaction
            .execute(
                "DELETE FROM runtime_bindings WHERE agent_id = ?1",
                [actor_id],
            )
            .await?;
        transaction
            .execute("DELETE FROM agents WHERE actor_id = ?1", [actor_id])
            .await?;
        transaction
            .execute(
                "DELETE FROM agent_wake_state WHERE agent_id = ?1",
                [actor_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM inbox_batch_items
                 WHERE batch_id IN (SELECT id FROM inbox_batches WHERE agent_id = ?1)",
                [actor_id],
            )
            .await?;
        transaction
            .execute("DELETE FROM inbox_batches WHERE agent_id = ?1", [actor_id])
            .await?;
        // The actor set changed; reuse actor_created so clients re-pull actors.
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
        Ok(())
    }

    /// Return the stable User for one handle, creating it when absent.
    pub async fn ensure_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        self.assert_open()?;
        require_non_empty("handle", handle)?;
        require_non_empty("display_name", display_name)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        if let Some(actor) = find_actor_by_handle(&transaction, handle).await? {
            if actor.kind != ActorKind::User {
                return Err(CollabError::InvalidArgument(format!(
                    "actor handle '{handle}' belongs to an Agent"
                )));
            }
            if actor.display_name != display_name {
                // Only leftover default names migrate. A custom display name
                // is user-owned and must survive later OS-username ensure.
                if actor.display_name != "Local User" {
                    transaction.commit().await?;
                    return Ok(actor);
                }
                // Explicit display-name migration on the stable handle: the
                // actor id, Memberships and Tasks are untouched.
                transaction
                    .execute(
                        "UPDATE actors SET display_name = ?2 WHERE id = ?1",
                        (actor.id.as_str(), display_name),
                    )
                    .await?;
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
                return Ok(Actor {
                    display_name: display_name.to_owned(),
                    ..actor
                });
            }
            transaction.commit().await?;
            return Ok(actor);
        }

        let actor = Actor {
            id: new_id(),
            kind: ActorKind::User,
            handle: handle.to_owned(),
            display_name: display_name.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                 VALUES (?1, 'user', ?2, ?3, ?4)",
                (
                    actor.id.as_str(),
                    actor.handle.as_str(),
                    actor.display_name.as_str(),
                    now,
                ),
            )
            .await?;
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
