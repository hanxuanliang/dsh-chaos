use crate::test_support::*;
use crate::{ChangeKind, CollabError, Result, SendMessageRequest, TargetKind};

#[tokio::test]
async fn follow_and_unfollow_are_idempotent_notifications() -> Result<()> {
    let World {
        core,
        user: owner,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: owner.id.clone(),
            client_request_id: "thread-follow-root".into(),
            text: "Open a Thread".into(),
        })
        .await?;
    let thread = core.create_thread(&root.message.id, &owner.id).await?;

    let before_follow = core.snapshot(&alpha.id).await?.cursor;
    core.follow_thread(&thread.id, &alpha.id).await?;
    core.follow_thread(&thread.id, &alpha.id).await?;
    let follow_changes = core.list_changes(&alpha.id, before_follow, 50).await?;
    assert_eq!(
        follow_changes
            .iter()
            .filter(|change| change.kind == ChangeKind::ThreadFollowChanged)
            .count(),
        1
    );

    let before_unfollow = core.snapshot(&alpha.id).await?.cursor;
    core.unfollow_thread(&thread.id, &alpha.id).await?;
    core.unfollow_thread(&thread.id, &alpha.id).await?;
    let unfollow_changes = core.list_changes(&alpha.id, before_unfollow, 50).await?;
    assert_eq!(
        unfollow_changes
            .iter()
            .filter(|change| change.kind == ChangeKind::ThreadFollowChanged)
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn summaries_order_distinct_repliers_by_their_latest_message() -> Result<()> {
    let World {
        core,
        user: owner,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: owner.id.clone(),
            client_request_id: "thread-summary-root".into(),
            text: "Summarize replies".into(),
        })
        .await?;
    let thread = core.create_thread(&root.message.id, &owner.id).await?;
    for (author_id, request_id) in [
        (&alpha.id, "summary-alpha-1"),
        (&beta.id, "summary-beta-1"),
        (&owner.id, "summary-owner-1"),
        (&alpha.id, "summary-alpha-2"),
    ] {
        core.send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: author_id.clone(),
            client_request_id: request_id.into(),
            text: request_id.into(),
        })
        .await?;
    }

    let summaries = core
        .thread_summaries(&owner.id, std::slice::from_ref(&root.message.id))
        .await?;
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].reply_count, 4);
    assert_eq!(
        summaries[0].recent_replier_ids,
        vec![alpha.id, owner.id, beta.id]
    );
    Ok(())
}

#[tokio::test]
async fn thread_inherits_parent_access_and_delivers_only_to_followers() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
    let outsider = core.create_user("thread-outsider", "Outsider").await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "thread-root".into(),
            text: "review in a thread".into(),
        })
        .await?;
    let beta_binding = core
        .bind_runtime(
            &beta.id,
            "thread-beta-session",
            "openai",
            "codex",
            "default",
        )
        .await?;
    let root_batch = core
        .check_inbox(
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
            10,
        )
        .await?;
    core.mark_model_seen(
        root_batch.id.as_deref().expect("root delivery batch"),
        &beta.id,
        beta_binding.generation,
        &beta_binding.session_id,
    )
    .await?;
    let beta_change_cursor = core.snapshot(&beta.id).await?.cursor;
    let thread = core.create_thread(&root.message.id, &alpha.id).await?;
    assert_eq!(thread.kind, TargetKind::Thread);
    assert_eq!(
        thread.parent_target_id.as_deref(),
        Some(channel.id.as_str())
    );
    assert_eq!(
        thread.root_message_id.as_deref(),
        Some(root.message.id.as_str())
    );
    assert_eq!(
        core.create_thread(&root.message.id, &beta.id).await?,
        thread
    );
    let beta_thread_changes = core.list_changes(&beta.id, beta_change_cursor, 50).await?;
    assert!(beta_thread_changes.iter().any(|change| {
        change.kind == ChangeKind::TargetCreated && change.entity_id == thread.id
    }));

    let first_reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "thread-reply-1".into(),
            text: "alpha reply".into(),
        })
        .await?;
    assert_eq!(first_reply.recipient_ids, vec![user.id.clone()]);
    assert_eq!(
        core.snapshot(&alpha.id).await?.followed_thread_ids,
        vec![thread.id.clone()]
    );
    assert!(
        core.snapshot(&beta.id)
            .await?
            .followed_thread_ids
            .is_empty()
    );

    core.follow_thread(&thread.id, &beta.id).await?;
    assert_eq!(
        core.snapshot(&beta.id).await?.followed_thread_ids,
        vec![thread.id.clone()]
    );
    let second_reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "thread-reply-2".into(),
            text: "owner reply".into(),
        })
        .await?;
    assert_eq!(
        second_reply.recipient_ids,
        vec![alpha.id.clone(), beta.id.clone()]
    );
    assert!(
        core.list_pending_wakes(10)
            .await?
            .iter()
            .any(|wake| wake.binding.agent_id == beta.id)
    );
    let thread_batch = core
        .check_inbox(
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
            10,
        )
        .await?;
    assert_eq!(thread_batch.messages.len(), 1);
    assert_eq!(thread_batch.messages[0].message.id, second_reply.message.id);
    core.mark_model_seen(
        thread_batch.id.as_deref().expect("Thread delivery batch"),
        &beta.id,
        beta_binding.generation,
        &beta_binding.session_id,
    )
    .await?;

    core.unfollow_thread(&thread.id, &beta.id).await?;
    assert!(
        core.snapshot(&beta.id)
            .await?
            .followed_thread_ids
            .is_empty()
    );
    let beta_unfollowed_cursor = core.snapshot(&beta.id).await?.cursor;
    assert_eq!(
        core.read_message(&beta.id, &thread.id, &second_reply.message.id)
            .await?,
        second_reply.message
    );
    let after_unfollow = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "thread-reply-3".into(),
            text: "beta should not receive this".into(),
        })
        .await?;
    assert_eq!(after_unfollow.recipient_ids, vec![user.id.clone()]);
    let beta_realtime_changes = core
        .list_changes(&beta.id, beta_unfollowed_cursor, 50)
        .await?;
    assert!(beta_realtime_changes.iter().any(|change| {
        change.kind == ChangeKind::MessageCreated && change.entity_id == after_unfollow.message.id
    }));

    let beta_reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: beta.id.clone(),
            client_request_id: "thread-reply-4".into(),
            text: "participating follows again".into(),
        })
        .await?;
    let mut expected_beta_reply_recipients = vec![user.id.clone(), alpha.id.clone()];
    expected_beta_reply_recipients.sort();
    assert_eq!(beta_reply.recipient_ids, expected_beta_reply_recipients);
    let final_reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: alpha.id.clone(),
            client_request_id: "thread-reply-5".into(),
            text: "beta follows again".into(),
        })
        .await?;
    assert!(final_reply.recipient_ids.contains(&beta.id));
    assert_eq!(
        core.snapshot(&beta.id).await?.followed_thread_ids,
        vec![thread.id.clone()]
    );
    assert!(matches!(
        core.read_messages(&outsider.id, &thread.id, 0, 10).await,
        Err(CollabError::PermissionDenied { .. })
    ));
    assert!(matches!(
        core.create_task(&first_reply.message.id, &alpha.id).await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.create_thread(&first_reply.message.id, &alpha.id).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}
