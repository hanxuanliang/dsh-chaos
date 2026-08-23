use super::test_support::*;

#[tokio::test]
async fn direct_target_is_unique_for_an_unordered_actor_pair() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        ..
    } = World::create().await?;
    let direct = core.create_direct(&alpha.id, &beta.id).await?;
    let reversed = core.create_direct(&beta.id, &alpha.id).await?;
    assert_eq!(direct, reversed);
    assert_eq!(direct.kind, TargetKind::Direct);
    assert!(direct.parent_target_id.is_none());
    assert!(direct.root_message_id.is_none());

    let sent = core
        .send_message(SendMessageRequest {
            target_id: direct.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "direct-message".into(),
            text: "only beta receives this".into(),
        })
        .await?;
    assert_eq!(sent.recipient_ids, vec![beta.id.clone()]);
    assert_eq!(
        core.read_messages(&beta.id, &direct.id, 0, 10).await?,
        vec![sent.message]
    );
    assert!(matches!(
        core.read_messages(&user.id, &direct.id, 0, 10).await,
        Err(CollabError::PermissionDenied { .. })
    ));
    assert!(matches!(
        core.add_member(&direct.id, &user.id, &alpha.id).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}

#[tokio::test]
async fn channel_details_are_owner_managed_versioned_identity_context() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;

    let context = core.identity_context(&alpha.id, Some(&channel.id)).await?;
    assert_eq!(
        context
            .target
            .as_ref()
            .map(|target| target.description.as_str()),
        Some("Design collaboration")
    );

    assert!(matches!(
        core.update_channel(
            &channel.id,
            &alpha.id,
            "Renamed",
            "A new purpose",
            channel.version,
        )
        .await,
        Err(CollabError::PermissionDenied { .. })
    ));
    assert!(matches!(
        core.archive_channel(&channel.id, &beta.id, channel.version)
            .await,
        Err(CollabError::PermissionDenied { .. })
    ));

    let updated = core
        .update_channel(
            &channel.id,
            &user.id,
            "product-design",
            "Review product interaction and accessibility",
            channel.version,
        )
        .await?;
    assert_eq!(updated.version, channel.version + 1);
    assert_eq!(updated.name, "product-design");
    assert_eq!(
        updated.description,
        "Review product interaction and accessibility"
    );
    let refreshed = core.identity_context(&alpha.id, Some(&channel.id)).await?;
    assert_eq!(refreshed.target.as_ref(), Some(&updated));

    assert!(matches!(
        core.update_channel(
            &channel.id,
            &user.id,
            "stale",
            "Stale update",
            channel.version,
        )
        .await,
        Err(CollabError::TargetVersionConflict { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn archived_channel_is_read_only_until_owner_restores_it() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "archive-root".into(),
            text: "Preserve this history".into(),
        })
        .await?
        .message;
    let task = core.create_task(&root.id, &user.id).await?;
    let thread = core.create_thread(&root.id, &user.id).await?;

    let archived = core
        .archive_channel(&channel.id, &user.id, channel.version)
        .await?;
    assert_eq!(archived.lifecycle, TargetLifecycle::Archived);
    assert_eq!(
        core.read_message(&alpha.id, &channel.id, &root.id).await?,
        root
    );

    let send = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "archive-write".into(),
            text: "must fail".into(),
        })
        .await;
    assert!(matches!(send, Err(CollabError::TargetNotWritable { .. })));
    assert!(matches!(
        core.add_member(&channel.id, &beta.id, &user.id).await,
        Err(CollabError::TargetNotWritable { .. })
    ));
    assert!(matches!(
        core.create_thread(&root.id, &user.id).await,
        Err(CollabError::TargetNotWritable { .. })
    ));
    assert!(matches!(
        core.follow_thread(&thread.id, &alpha.id).await,
        Err(CollabError::TargetNotWritable { .. })
    ));
    assert!(matches!(
        core.claim_task(&task.message_id, &alpha.id).await,
        Err(CollabError::TargetNotWritable { .. })
    ));

    let restored = core
        .restore_channel(&channel.id, &user.id, archived.version)
        .await?;
    assert_eq!(restored.lifecycle, TargetLifecycle::Active);
    core.send_message(SendMessageRequest {
        target_id: channel.id.clone(),
        author_id: user.id.clone(),
        client_request_id: "restored-write".into(),
        text: "writes work again".into(),
    })
    .await?;
    core.follow_thread(&thread.id, &alpha.id).await?;
    Ok(())
}

#[tokio::test]
async fn deleted_channel_is_hidden_but_exact_history_and_pending_delivery_survive() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let outsider = core
        .create_user("history-outsider", "History Outsider")
        .await?;
    let binding = core
        .bind_runtime(
            &alpha.id,
            "deleted-history-session",
            "openai",
            "codex",
            "default",
        )
        .await?;
    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "deleted-history-message".into(),
            text: "retain this delivery".into(),
        })
        .await?
        .message;
    let thread = core.create_thread(&sent.id, &user.id).await?;
    core.follow_thread(&thread.id, &alpha.id).await?;

    let deleted = core
        .delete_channel(&channel.id, &user.id, channel.version)
        .await?;
    assert_eq!(deleted.lifecycle, TargetLifecycle::Deleted);
    assert!(
        core.snapshot(&alpha.id)
            .await?
            .targets
            .iter()
            .all(|target| target.id != channel.id && target.id != thread.id)
    );
    assert!(
        core.snapshot(&alpha.id)
            .await?
            .followed_thread_ids
            .is_empty()
    );
    assert_eq!(
        core.read_message(&alpha.id, &channel.id, &sent.id).await?,
        sent
    );
    assert!(matches!(
        core.read_message(&outsider.id, &channel.id, &sent.id).await,
        Err(CollabError::PermissionDenied { .. })
    ));

    let wakes = core.list_pending_wakes(10).await?;
    assert!(wakes.iter().any(|wake| wake.binding.agent_id == alpha.id));
    let batch = core
        .check_inbox(&alpha.id, binding.generation, &binding.session_id, 10)
        .await?;
    assert!(batch.messages.iter().any(|item| item.message.id == sent.id));
    let context = batch
        .contexts
        .iter()
        .find(|context| {
            context
                .target
                .as_ref()
                .is_some_and(|target| target.id == channel.id)
        })
        .expect("deleted target identity context");
    assert_eq!(
        context.target.as_ref().map(|target| target.lifecycle),
        Some(TargetLifecycle::Deleted)
    );
    assert_eq!(
        context
            .target
            .as_ref()
            .map(|target| target.description.as_str()),
        Some("Design collaboration")
    );
    assert!(context.members.is_empty());
    Ok(())
}
