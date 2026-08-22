use crate::test_support::*;

#[tokio::test]
async fn snapshot_and_change_cursor_are_authorization_filtered() -> Result<()> {
    let core = CollabCore::open_memory().await?;
    let owner = core.ensure_user("cursor-owner", "Owner").await?;
    // A repeated ensure with the current name is a no-op returning the
    // same actor; a different name is an explicit migration (covered by
    // ensure_user_migrates_display_name_on_stable_handle).
    assert_eq!(core.ensure_user("cursor-owner", "Owner").await?, owner);
    let alpha = core
        .create_agent("cursor-alpha", "Alpha", "/tmp/cursor-alpha")
        .await?;
    let outsider = core.ensure_user("cursor-outsider", "Outsider").await?;
    let channel = core.create_channel("cursor-channel", &owner.id).await?;
    core.add_member(&channel.id, &owner.id, &owner.id).await?;
    core.add_member(&channel.id, &alpha.id, &owner.id).await?;

    let owner_snapshot = core.snapshot(&owner.id).await?;
    assert_eq!(owner_snapshot.actor, owner);
    assert_eq!(owner_snapshot.targets, vec![channel.clone()]);
    assert!(owner_snapshot.followed_thread_ids.is_empty());
    assert!(owner_snapshot.tasks.is_empty());
    assert!(core.snapshot(&outsider.id).await?.targets.is_empty());

    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: owner.id.clone(),
            client_request_id: "cursor-message".into(),
            text: "durable change".into(),
        })
        .await?;
    let owner_changes = core
        .list_changes(&owner.id, owner_snapshot.cursor, 50)
        .await?;
    assert_eq!(owner_changes.len(), 1);
    assert_eq!(owner_changes[0].kind, ChangeKind::MessageCreated);
    assert_eq!(
        owner_changes[0].target_id.as_deref(),
        Some(channel.id.as_str())
    );
    assert_eq!(owner_changes[0].entity_id, sent.message.id);
    assert_eq!(
        core.list_changes(&alpha.id, owner_snapshot.cursor, 50)
            .await?,
        owner_changes
    );
    assert!(
        core.list_changes(&outsider.id, owner_snapshot.cursor, 50)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn change_retention_requires_snapshot_resync_outside_retained_range() -> Result<()> {
    let (core, user, _alpha, _beta, channel) = fixture().await?;
    let cursor = core.snapshot(&user.id).await?.cursor;
    assert!(cursor > 0);

    let floor = core.prune_changes_before(now_ms()? + 1).await?;
    assert_eq!(floor, cursor);
    assert_eq!(core.snapshot(&user.id).await?.cursor, floor);
    assert!(core.list_changes(&user.id, floor, 10).await?.is_empty());
    for after_seq in [floor - 1, floor + 1] {
        assert!(matches!(
            core.list_changes(&user.id, after_seq, 10).await,
            Err(CollabError::ChangeCursorOutOfRange {
                minimum_cursor,
                maximum_cursor,
                ..
            }) if minimum_cursor == floor && maximum_cursor == floor
        ));
    }

    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: user.id.clone(),
            client_request_id: "after-retention".into(),
            text: "new retained change".into(),
        })
        .await?;
    let changes = core.list_changes(&user.id, floor, 10).await?;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].entity_id, sent.message.id);
    assert_eq!(core.prune_changes_before(0).await?, floor);
    Ok(())
}
