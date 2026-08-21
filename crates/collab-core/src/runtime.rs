//! Stable Agent to DSH Session generation bindings.

use super::*;

impl CollabCore {
    /// Bind a new DSH Session generation to a stable Agent.
    pub async fn bind_runtime(
        &self,
        agent_id: &str,
        session_id: &str,
        provider: &str,
        model: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        self.assert_open()?;
        for (name, value) in [
            ("agent_id", agent_id),
            ("session_id", session_id),
            ("provider", provider),
            ("model", model),
            ("preset", preset),
        ] {
            require_non_empty(name, value)?;
        }

        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_agent(&transaction, agent_id).await?;
        let generation = current_generation(&transaction, agent_id).await? + 1;
        transaction
            .execute(
                "INSERT INTO runtime_bindings
                 (agent_id, session_id, generation, provider, model, preset, bound_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(agent_id) DO UPDATE SET
                   session_id = excluded.session_id,
                   generation = excluded.generation,
                   provider = excluded.provider,
                   model = excluded.model,
                   preset = excluded.preset,
                   bound_at_ms = excluded.bound_at_ms",
                (
                    agent_id, session_id, generation, provider, model, preset, now,
                ),
            )
            .await?;
        transaction.commit().await?;

        Ok(RuntimeBinding {
            agent_id: agent_id.to_owned(),
            session_id: session_id.to_owned(),
            generation,
            provider: provider.to_owned(),
            model: model.to_owned(),
            preset: preset.to_owned(),
            bound_at_ms: now,
        })
    }

    /// Return the current runtime binding for one stable Agent.
    pub async fn runtime_binding(&self, agent_id: &str) -> Result<Option<RuntimeBinding>> {
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        let connection = self.connection.lock().await;
        find_runtime_binding(&connection, "agent_id", agent_id).await
    }

    /// Replace only the preset label of one exact runtime generation.
    ///
    /// Used to migrate the former chaos `default` sentinel after the official
    /// DSH roster resolves it. Session identity, generation, and wake fencing
    /// stay unchanged.
    pub async fn update_runtime_preset(
        &self,
        agent_id: &str,
        generation: i64,
        session_id: &str,
        preset: &str,
    ) -> Result<RuntimeBinding> {
        self.assert_open()?;
        require_non_empty("agent_id", agent_id)?;
        require_non_empty("session_id", session_id)?;
        require_non_empty("preset", preset)?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let mut rows = transaction
            .query(
                "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
                 FROM runtime_bindings WHERE agent_id = ?1",
                [agent_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::NotFound {
                entity: "runtime binding",
                id: agent_id.to_owned(),
            });
        };
        let current = RuntimeBinding {
            agent_id: row.get(0)?,
            session_id: row.get(1)?,
            generation: row.get(2)?,
            provider: row.get(3)?,
            model: row.get(4)?,
            preset: row.get(5)?,
            bound_at_ms: row.get(6)?,
        };
        drop(rows);
        if current.generation != generation || current.session_id != session_id {
            return Err(CollabError::RuntimeGenerationMismatch {
                agent_id: agent_id.to_owned(),
            });
        }
        transaction
            .execute(
                "UPDATE runtime_bindings SET preset = ?1
                 WHERE agent_id = ?2 AND generation = ?3 AND session_id = ?4",
                (preset, agent_id, generation, session_id),
            )
            .await?;
        transaction.commit().await?;
        Ok(RuntimeBinding {
            preset: preset.to_owned(),
            ..current
        })
    }

    /// Resolve the stable Agent identity that owns one live DSH Session id.
    pub async fn runtime_binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RuntimeBinding>> {
        self.assert_open()?;
        require_non_empty("session_id", session_id)?;
        let connection = self.connection.lock().await;
        find_runtime_binding(&connection, "session_id", session_id).await
    }

    /// List every durable current runtime binding for process recovery.
    pub async fn list_runtime_bindings(&self) -> Result<Vec<RuntimeBinding>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
                 FROM runtime_bindings ORDER BY agent_id",
                (),
            )
            .await?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().await? {
            bindings.push(RuntimeBinding {
                agent_id: row.get(0)?,
                session_id: row.get(1)?,
                generation: row.get(2)?,
                provider: row.get(3)?,
                model: row.get(4)?,
                preset: row.get(5)?,
                bound_at_ms: row.get(6)?,
            });
        }
        Ok(bindings)
    }
}

pub(crate) async fn find_runtime_binding(
    connection: &Connection,
    key: &'static str,
    value: &str,
) -> Result<Option<RuntimeBinding>> {
    let statement = match key {
        "agent_id" => {
            "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
             FROM runtime_bindings WHERE agent_id = ?1"
        }
        "session_id" => {
            "SELECT agent_id, session_id, generation, provider, model, preset, bound_at_ms
             FROM runtime_bindings WHERE session_id = ?1"
        }
        _ => {
            return Err(CollabError::Database(
                "unsupported runtime binding lookup key".into(),
            ));
        }
    };
    let mut rows = connection.query(statement, [value]).await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(RuntimeBinding {
        agent_id: row.get(0)?,
        session_id: row.get(1)?,
        generation: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        preset: row.get(5)?,
        bound_at_ms: row.get(6)?,
    }))
}

pub(crate) async fn current_generation(connection: &Connection, agent_id: &str) -> Result<i64> {
    let mut rows = connection
        .query(
            "SELECT generation FROM runtime_bindings WHERE agent_id = ?1",
            [agent_id],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Ok(0),
    }
}

pub(crate) async fn require_current_binding(
    connection: &Connection,
    agent_id: &str,
    generation: i64,
    session_id: &str,
) -> Result<()> {
    let mut rows = connection
        .query(
            "SELECT session_id, generation FROM runtime_bindings WHERE agent_id = ?1",
            [agent_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("runtime binding", agent_id));
    };
    let current_session = row.get::<String>(0)?;
    let current_generation = row.get::<i64>(1)?;
    if current_session != session_id || current_generation != generation {
        return Err(CollabError::RuntimeGenerationMismatch {
            agent_id: agent_id.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn runtime_lookup_keeps_session_ownership_unique() -> Result<()> {
        let (core, _user, alpha, beta, _channel) = fixture().await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-owned", "openai", "codex", "default")
            .await?;

        assert_eq!(
            core.runtime_binding(&alpha.id).await?,
            Some(binding.clone())
        );
        assert_eq!(
            core.runtime_binding_for_session("session-owned").await?,
            Some(binding)
        );
        assert!(
            core.bind_runtime(&beta.id, "session-owned", "openai", "codex", "default")
                .await
                .is_err()
        );
        assert_eq!(core.list_runtime_bindings().await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_preset_migration_preserves_session_generation() -> Result<()> {
        let (core, _user, alpha, _beta, _channel) = fixture().await?;
        let binding = core
            .bind_runtime(&alpha.id, "session-legacy", "default", "default", "default")
            .await?;
        let migrated = core
            .update_runtime_preset(
                &alpha.id,
                binding.generation,
                &binding.session_id,
                "standard",
            )
            .await?;
        assert_eq!(migrated.session_id, binding.session_id);
        assert_eq!(migrated.generation, binding.generation);
        assert_eq!(migrated.bound_at_ms, binding.bound_at_ms);
        assert_eq!(migrated.preset, "standard");
        assert_eq!(core.runtime_binding(&alpha.id).await?, Some(migrated));

        let stale = core
            .update_runtime_preset(
                &alpha.id,
                binding.generation + 1,
                "session-legacy",
                "minimal",
            )
            .await;
        assert!(matches!(
            stale,
            Err(CollabError::RuntimeGenerationMismatch { .. })
        ));
        Ok(())
    }
}
