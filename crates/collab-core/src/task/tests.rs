use crate::test_support::*;
use crate::{ChangeKind, CollabError, Result, SendMessageRequest, TaskStatus};

#[tokio::test]
async fn task_reads_carry_authoritative_anchor_text() -> Result<()> {
    let (core, user, alpha, _beta, channel) = fixture().await?;
    let anchor = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "anchor-1".into(),
            text: "anchor body survives paging".into(),
        })
        .await?
        .message;
    let created = core.create_task(&anchor.id, &user.id).await?;
    assert_eq!(
        created.anchor_text.as_deref(),
        Some("anchor body survives paging")
    );

    // Push the anchor far outside any recent-message page; the Task read
    // still resolves the true anchor body from the store.
    for index in 0..120 {
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: format!("filler-{index}"),
            text: format!("filler {index}"),
        })
        .await?;
    }
    let tasks = core.list_tasks(&alpha.id, Some(&channel.id)).await?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].anchor_text.as_deref(),
        Some("anchor body survives paging")
    );

    // Mutations keep the anchor attached.
    let claimed = core.claim_task(&anchor.id, &alpha.id).await?;
    assert_eq!(
        claimed.anchor_text.as_deref(),
        Some("anchor body survives paging")
    );
    Ok(())
}

#[tokio::test]
async fn task_lifecycle_uses_version_fencing_and_emits_changes() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
    let before = core.snapshot(&user.id).await?.cursor;
    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "task-lifecycle-message".into(),
            text: "finish the lifecycle".into(),
        })
        .await?;
    let created = core.create_task(&sent.message.id, &user.id).await?;
    let claimed = core.claim_task(&sent.message.id, &alpha.id).await?;
    assert_eq!(claimed.version, created.version + 1);
    assert!(matches!(
        core.update_task_status(
            &sent.message.id,
            &beta.id,
            TaskStatus::InReview,
            claimed.version,
        )
        .await,
        Err(CollabError::PermissionDenied { .. })
    ));

    let owner_review = core
        .update_task_status(
            &sent.message.id,
            &user.id,
            TaskStatus::InReview,
            claimed.version,
        )
        .await?;
    let reopened = core
        .update_task_status(
            &sent.message.id,
            &alpha.id,
            TaskStatus::InProgress,
            owner_review.version,
        )
        .await?;
    let review = core
        .update_task_status(
            &sent.message.id,
            &alpha.id,
            TaskStatus::InReview,
            reopened.version,
        )
        .await?;
    assert!(matches!(
        core.update_task_status(
            &sent.message.id,
            &alpha.id,
            TaskStatus::Done,
            claimed.version,
        )
        .await,
        Err(CollabError::TaskVersionConflict { .. })
    ));
    let unclaimed = core
        .unclaim_task(&sent.message.id, &alpha.id, review.version)
        .await?;
    assert_eq!(unclaimed.status, TaskStatus::InReview);
    assert!(unclaimed.assignee_id.is_none());

    let beta_claim = core.claim_task(&sent.message.id, &beta.id).await?;
    let beta_review = core
        .update_task_status(
            &sent.message.id,
            &beta.id,
            TaskStatus::InReview,
            beta_claim.version,
        )
        .await?;
    let done = core
        .update_task_status(
            &sent.message.id,
            &beta.id,
            TaskStatus::Done,
            beta_review.version,
        )
        .await?;
    assert!(matches!(
        core.unclaim_task(&sent.message.id, &beta.id, done.version)
            .await,
        Err(CollabError::TaskTransitionDenied { .. })
    ));
    assert_eq!(
        core.list_tasks(&user.id, Some(&channel.id)).await?,
        vec![done]
    );
    let changes = core.list_changes(&user.id, before, 50).await?;
    assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
    assert_eq!(
        changes
            .iter()
            .filter(|change| change.kind == ChangeKind::TaskCreated)
            .count(),
        1
    );
    assert_eq!(
        changes
            .iter()
            .filter(|change| change.kind == ChangeKind::TaskUpdated)
            .count(),
        8
    );
    Ok(())
}

#[tokio::test]
async fn only_one_concurrent_task_claim_wins() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: user.id.clone(),
            client_request_id: "task-message".into(),
            text: "implement it".into(),
        })
        .await?;
    core.create_task(&sent.message.id, &user.id).await?;

    let alpha_claim = core.claim_task(&sent.message.id, &alpha.id);
    let beta_claim = core.claim_task(&sent.message.id, &beta.id);
    let (alpha_result, beta_result) = tokio::join!(alpha_claim, beta_claim);
    let successes = usize::from(alpha_result.is_ok()) + usize::from(beta_result.is_ok());
    let conflicts = usize::from(matches!(
        alpha_result,
        Err(CollabError::TaskAlreadyClaimed { .. })
    )) + usize::from(matches!(
        beta_result,
        Err(CollabError::TaskAlreadyClaimed { .. })
    ));
    assert_eq!(successes, 1);
    assert_eq!(conflicts, 1);
    Ok(())
}
