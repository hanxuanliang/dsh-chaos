use super::test_support::*;

#[tokio::test]
async fn runtime_lookup_keeps_session_ownership_unique() -> Result<()> {
    let World {
        core, alpha, beta, ..
    } = World::create().await?;
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
    let World { core, alpha, .. } = World::create().await?;
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
