use super::test_support::*;
use crate::{
    AgentCharter, AgentLifecycle, ChangeKind, CollabCore, CollabError, Result, SendMessageRequest,
};

use turso::Connection;

async fn count_where(
    connection: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<i64> {
    let allowed = [
        ("memberships", "actor_id"),
        ("runtime_bindings", "agent_id"),
        ("agents", "actor_id"),
        ("agent_wake_state", "agent_id"),
        ("inbox_batches", "agent_id"),
        ("actors", "id"),
        ("messages", "author_id"),
    ];
    assert!(allowed.contains(&(table, column)));
    let mut rows = connection
        .query(
            format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
            [value],
        )
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
    Ok(row.get(0)?)
}

#[tokio::test]
async fn agent_profile_normalizes_charter_and_fences_updates() -> Result<()> {
    let core = CollabCore::open_memory().await?;
    let owner = core.create_user("owner", "Owner").await?;
    let profile = core
        .create_agent_profile(
            "grok老马melody",
            "grok老马melody",
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
    assert_eq!(updated.actor.handle, "grok老马melody");
    assert_eq!(updated.actor.display_name, "Frontend Reviewer");
    assert_eq!(updated.workspace_path, profile.workspace_path);
    assert!(matches!(
        core.update_agent_profile(
            &profile.actor.id,
            "Stale",
            AgentCharter::default(),
            profile.version
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
    let World {
        core,
        user,
        alpha,
        beta,
        ..
    } = World::create().await?;
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
async fn agent_avatar_is_validated_versioned_and_visible_in_actor_projections() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let profile = core.agent_profile(&alpha.id).await?;
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    let updated = core
        .update_agent_avatar(&alpha.id, Some(png), profile.version)
        .await?;
    assert_eq!(updated.version, profile.version + 1);
    assert_eq!(updated.actor.avatar_data_url.as_deref(), Some(png));

    let actors = core.list_actors(&user.id).await?;
    assert_eq!(
        actors
            .iter()
            .find(|actor| actor.id == alpha.id)
            .and_then(|actor| actor.avatar_data_url.as_deref()),
        Some(png)
    );
    let members = core.list_target_members(&user.id, &channel.id).await?;
    assert_eq!(
        members
            .iter()
            .find(|actor| actor.id == alpha.id)
            .and_then(|actor| actor.avatar_data_url.as_deref()),
        Some(png)
    );

    assert!(matches!(
        core.update_agent_avatar(&alpha.id, Some(png), profile.version)
            .await,
        Err(CollabError::AgentProfileVersionConflict {
            expected: 1,
            actual: 2,
            ..
        })
    ));
    assert!(matches!(
        core.update_agent_avatar(
            &alpha.id,
            Some("data:image/png;base64,aGVsbG8="),
            updated.version,
        )
        .await,
        Err(CollabError::InvalidArgument(_))
    ));

    let cleared = core
        .update_agent_avatar(&alpha.id, None, updated.version)
        .await?;
    assert_eq!(cleared.version, updated.version + 1);
    assert_eq!(cleared.actor.avatar_data_url, None);
    Ok(())
}

#[tokio::test]
async fn delete_agent_removes_operational_state_but_keeps_history() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
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
    let World { core, user, .. } = World::create().await?;
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
