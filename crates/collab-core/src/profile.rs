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

pub(crate) fn normalize_charter_list(
    name: &str,
    values: Vec<String>,
    item_max: usize,
) -> Result<Vec<String>> {
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

pub(crate) async fn find_agent_profile(
    connection: &Connection,
    agent_id: &str,
) -> Result<AgentProfile> {
    let mut rows = connection
        .query(
            "SELECT actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms,
                    agent.workspace_path, agent.lifecycle, agent.charter_json,
                    agent.profile_version, agent.created_at_ms, agent.updated_at_ms
             FROM agents agent
             JOIN actors actor ON actor.id = agent.actor_id
             WHERE agent.actor_id = ?1",
            [agent_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("agent profile", agent_id));
    };
    agent_profile_from_row(&row)
}

pub(crate) async fn agent_profiles(connection: &Connection) -> Result<Vec<AgentProfile>> {
    let mut rows = connection
        .query(
            "SELECT actor.id, actor.kind, actor.handle, actor.display_name, actor.created_at_ms,
                    agent.workspace_path, agent.lifecycle, agent.charter_json,
                    agent.profile_version, agent.created_at_ms, agent.updated_at_ms
             FROM agents agent
             JOIN actors actor ON actor.id = agent.actor_id
             ORDER BY actor.handle, actor.id",
            (),
        )
        .await?;
    let mut profiles = Vec::new();
    while let Some(row) = rows.next().await? {
        profiles.push(agent_profile_from_row(&row)?);
    }
    Ok(profiles)
}

pub(crate) fn agent_profile_from_row(row: &Row) -> Result<AgentProfile> {
    let id = row.get::<String>(0)?;
    let kind_text = row.get::<String>(1)?;
    let kind = parse_actor_kind(&id, &kind_text)?;
    if kind != ActorKind::Agent {
        return Err(CollabError::Database(format!(
            "Agent Profile '{id}' belongs to a non-Agent actor"
        )));
    }
    let lifecycle_text = row.get::<String>(6)?;
    let charter_json = row.get::<String>(7)?;
    let lifecycle = parse_agent_lifecycle(&id, &lifecycle_text)?;
    let charter = decode_charter(&id, &charter_json)?;
    Ok(AgentProfile {
        actor: Actor {
            id,
            kind,
            handle: row.get(2)?,
            display_name: row.get(3)?,
            created_at_ms: row.get(4)?,
        },
        workspace_path: row.get(5)?,
        lifecycle,
        charter,
        version: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn agent_profile_normalizes_charter_and_fences_updates() -> Result<()> {
        let core = CollabCore::open_memory().await?;
        let owner = core.create_user("owner", "Owner").await?;
        let profile = core
            .create_agent_profile(
                "reviewer",
                "Reviewer",
                "/tmp/reviewer",
                AgentCharter {
                    schema_version: 1,
                    summary: "  Review frontend behavior.  ".into(),
                    capabilities: vec!["a11y".into(), " A11Y ".into(), "visual".into()],
                    constraints: vec!["Yield backend implementation".into()],
                },
            )
            .await?;
        assert_eq!(profile.version, 1);
        assert_eq!(profile.charter.summary, "Review frontend behavior.");
        assert_eq!(profile.charter.capabilities, vec!["a11y", "visual"]);
        assert_eq!(profile.lifecycle, AgentLifecycle::Active);

        let updated = core
            .update_agent_profile(
                &profile.actor.id,
                "Frontend Reviewer",
                AgentCharter {
                    schema_version: 1,
                    summary: "Own interaction review".into(),
                    capabilities: vec!["frontend".into()],
                    constraints: Vec::new(),
                },
                profile.version,
            )
            .await?;
        assert_eq!(updated.version, 2);
        assert_eq!(updated.actor.id, profile.actor.id);
        assert_eq!(updated.actor.handle, "reviewer");
        assert_eq!(updated.actor.display_name, "Frontend Reviewer");
        assert_eq!(updated.workspace_path, profile.workspace_path);
        assert!(matches!(
            core.update_agent_profile(
                &profile.actor.id,
                "Stale",
                AgentCharter::default(),
                profile.version,
            )
            .await,
            Err(CollabError::AgentProfileVersionConflict {
                expected: 1,
                actual: 2,
                ..
            })
        ));

        let changes = core.list_changes(&owner.id, 0, 20).await?;
        assert!(changes.iter().any(|change| {
            change.kind == ChangeKind::AgentProfileChanged && change.entity_id == profile.actor.id
        }));
        Ok(())
    }

    #[tokio::test]
    async fn agent_profile_directory_is_complete_and_handle_ordered() -> Result<()> {
        let (core, user, alpha, beta, _channel) = fixture().await?;
        let profiles = core.list_agent_profiles(&user.id).await?;
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.actor.id.as_str())
                .collect::<Vec<_>>(),
            vec![alpha.id.as_str(), beta.id.as_str()]
        );
        assert_eq!(profiles[0].actor.handle, "alpha");
        assert_eq!(profiles[0].workspace_path, "/tmp/alpha");
        assert_eq!(profiles[0].version, 1);
        assert_eq!(profiles[1].actor.handle, "beta");
        Ok(())
    }

    #[tokio::test]
    async fn delete_agent_removes_operational_state_but_keeps_history() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        core.create_direct(&user.id, &alpha.id).await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-delete", "openai", "codex", "default")
            .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "wake-alpha".into(),
            text: "wake up".into(),
        })
        .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "alpha-message".into(),
            text: "alpha was here".into(),
        })
        .await?;
        let batch = core
            .check_inbox(&alpha.id, binding.generation, &binding.session_id, 10)
            .await?;
        assert!(batch.id.is_some());
        let directory_before = core.list_actors(&user.id).await?;
        assert!(directory_before.iter().any(|actor| actor.id == alpha.id));
        let actors_before = {
            let connection = core.connection.lock().await;
            count(&connection, "actors").await?
        };

        core.delete_agent(&alpha.id).await?;

        // The deleted Agent leaves the directory but keeps its actors row and
        // stays readable as the author of its history messages.
        let directory_after = core.list_actors(&user.id).await?;
        assert!(!directory_after.iter().any(|actor| actor.id == alpha.id));
        assert!(directory_after.iter().any(|actor| actor.id == user.id));
        let history = core.read_messages(&user.id, &channel.id, 0, 10).await?;
        assert!(
            history
                .iter()
                .any(|message| message.author_id == alpha.id && message.text == "alpha was here")
        );

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "actors").await?, actors_before);
        assert_eq!(
            count_where(&connection, "memberships", "actor_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "runtime_bindings", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "agents", "actor_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "agent_wake_state", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(
            count_where(&connection, "inbox_batches", "agent_id", &alpha.id).await?,
            0
        );
        assert_eq!(count(&connection, "inbox_batch_items").await?, 0);
        assert_eq!(
            count_where(&connection, "actors", "id", &alpha.id).await?,
            1
        );
        assert_eq!(
            count_where(&connection, "messages", "author_id", &alpha.id).await?,
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_agent_rejects_users_and_missing_actors() -> Result<()> {
        let (core, user, _alpha, _beta, _channel) = fixture().await?;
        assert!(matches!(
            core.delete_agent(&user.id).await,
            Err(CollabError::NotFound {
                entity: "agent",
                ..
            })
        ));
        assert!(matches!(
            core.delete_agent("missing-actor").await,
            Err(CollabError::NotFound {
                entity: "actor",
                ..
            })
        ));
        Ok(())
    }
}
