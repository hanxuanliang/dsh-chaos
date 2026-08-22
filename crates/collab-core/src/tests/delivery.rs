use super::test_support::*;

#[tokio::test]
async fn runtime_generation_fences_model_seen_receipts() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let first_binding = core
        .bind_runtime(&alpha.id, "session-1", "openai", "codex", "default")
        .await?;
    core.send_message(SendMessageRequest {
        target_id: channel.id.clone(),
        author_id: user.id.clone(),
        client_request_id: "generation-message".into(),
        text: "read me".into(),
    })
    .await?;
    let batch = core
        .check_inbox(
            &alpha.id,
            first_binding.generation,
            &first_binding.session_id,
            10,
        )
        .await?;
    let batch_id = batch.id.expect("batch with one message");
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.contexts.len(), 1);
    assert_eq!(batch.contexts[0].agent.actor.id, alpha.id);
    assert_eq!(batch.contexts[0].target.as_ref(), Some(&channel));
    assert!(
        batch.contexts[0]
            .members
            .iter()
            .any(|member| member.actor.id == user.id && member.role == MembershipRole::Owner)
    );

    let second_binding = core
        .bind_runtime(&alpha.id, "session-2", "openai", "codex", "default")
        .await?;
    assert_eq!(second_binding.generation, first_binding.generation + 1);
    let stale = core
        .mark_model_seen(
            &batch_id,
            &alpha.id,
            first_binding.generation,
            &first_binding.session_id,
        )
        .await;
    assert!(matches!(
        stale,
        Err(CollabError::RuntimeGenerationMismatch { .. })
    ));

    let redelivered = core
        .check_inbox(
            &alpha.id,
            second_binding.generation,
            &second_binding.session_id,
            10,
        )
        .await?;
    assert_eq!(redelivered.messages.len(), 1);
    Ok(())
}

#[tokio::test]
async fn wake_watermark_is_level_triggered_and_generation_fenced() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let first = core
        .bind_runtime(&alpha.id, "wake-session-1", "openai", "codex", "default")
        .await?;
    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "wake-message".into(),
            text: "ring once".into(),
        })
        .await?;

    let wakes = core.list_pending_wakes(10).await?;
    assert_eq!(wakes.len(), 1);
    assert_eq!(wakes[0].binding, first);
    assert_eq!(wakes[0].pending_seq, sent.message.seq);
    core.mark_notified(
        &alpha.id,
        first.generation,
        &first.session_id,
        sent.message.seq,
    )
    .await?;
    assert!(core.list_pending_wakes(10).await?.is_empty());

    core.rearm_runtime_wake(&alpha.id, first.generation, &first.session_id)
        .await?;
    assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
    core.mark_notified(
        &alpha.id,
        first.generation,
        &first.session_id,
        sent.message.seq,
    )
    .await?;
    assert!(core.list_pending_wakes(10).await?.is_empty());

    {
        let connection = core.connection.lock().await;
        connection
            .execute(
                "UPDATE memberships SET left_at_ms = 1
                     WHERE target_id = ?1 AND actor_id = ?2",
                (channel.id.as_str(), alpha.id.as_str()),
            )
            .await?;
    }
    assert!(core.list_pending_wakes(10).await?.is_empty());
    core.add_member(&channel.id, &alpha.id, &user.id).await?;
    assert_eq!(core.list_pending_wakes(10).await?.len(), 1);
    core.mark_notified(
        &alpha.id,
        first.generation,
        &first.session_id,
        sent.message.seq,
    )
    .await?;
    assert!(core.list_pending_wakes(10).await?.is_empty());

    let second = core
        .bind_runtime(&alpha.id, "wake-session-2", "openai", "codex", "default")
        .await?;
    let rebound = core.list_pending_wakes(10).await?;
    assert_eq!(rebound.len(), 1);
    assert_eq!(rebound[0].binding, second);
    assert!(matches!(
        core.mark_notified(
            &alpha.id,
            first.generation,
            &first.session_id,
            sent.message.seq,
        )
        .await,
        Err(CollabError::RuntimeGenerationMismatch { .. })
    ));
    core.mark_notified(
        &alpha.id,
        second.generation,
        &second.session_id,
        sent.message.seq,
    )
    .await?;

    let batch = core
        .check_inbox(&alpha.id, second.generation, &second.session_id, 10)
        .await?;
    core.mark_model_seen(
        batch.id.as_deref().expect("one-message batch"),
        &alpha.id,
        second.generation,
        &second.session_id,
    )
    .await?;
    core.bind_runtime(&alpha.id, "wake-session-3", "openai", "codex", "default")
        .await?;
    assert!(core.list_pending_wakes(10).await?.is_empty());
    Ok(())
}
