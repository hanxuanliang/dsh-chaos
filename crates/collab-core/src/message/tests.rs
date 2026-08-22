use super::model::SendFailpoint;
use crate::test_support::*;
use crate::{CollabCore, CollabError, Result, SendMessageRequest};

#[tokio::test]
async fn send_commits_message_deliveries_and_wakes_together() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
    let result = core
        .send_message(SendMessageRequest {
            target_id: channel.id,
            author_id: user.id,
            client_request_id: "send-1".into(),
            text: "review this".into(),
        })
        .await?;

    assert_eq!(
        result.recipient_ids,
        vec![alpha.id.clone(), beta.id.clone()]
    );
    assert_eq!(
        result.wake_agent_ids,
        vec![alpha.id.clone(), beta.id.clone()]
    );
    assert!(!result.replayed);

    let connection = core.connection.lock().await;
    assert_eq!(count(&connection, "messages").await?, 1);
    assert_eq!(count(&connection, "deliveries").await?, 2);
    assert_eq!(count(&connection, "agent_wake_state").await?, 2);
    Ok(())
}

#[tokio::test]
async fn send_rolls_back_message_when_recipient_phase_fails() -> Result<()> {
    let World {
        core,
        user,
        channel,
        ..
    } = World::create().await?;
    let failure = core
        .send_message_inner(
            SendMessageRequest {
                target_id: channel.id,
                author_id: user.id,
                client_request_id: "send-fail".into(),
                text: "must roll back".into(),
            },
            SendFailpoint::AfterMessageInsert,
        )
        .await;
    assert!(matches!(failure, Err(CollabError::InjectedSendFailure)));

    let connection = core.connection.lock().await;
    assert_eq!(count(&connection, "messages").await?, 0);
    assert_eq!(count(&connection, "deliveries").await?, 0);
    assert_eq!(count(&connection, "agent_wake_state").await?, 0);
    Ok(())
}

#[tokio::test]
async fn repeated_send_request_is_idempotent() -> Result<()> {
    let World {
        core,
        user,
        channel,
        ..
    } = World::create().await?;
    let request = SendMessageRequest {
        target_id: channel.id,
        author_id: user.id,
        client_request_id: "same-request".into(),
        text: "only once".into(),
    };
    let first = core.send_message(request.clone()).await?;
    let replay = core.send_message(request).await?;
    assert_eq!(first.message, replay.message);
    assert!(replay.replayed);

    let connection = core.connection.lock().await;
    assert_eq!(count(&connection, "messages").await?, 1);
    assert_eq!(count(&connection, "deliveries").await?, 2);
    Ok(())
}

#[tokio::test]
async fn exact_target_reads_recheck_current_membership() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let outsider = core.create_user("outsider", "Outsider").await?;
    let sent = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "history-message".into(),
            text: "history".into(),
        })
        .await?;

    assert_eq!(
        core.read_message(&alpha.id, &channel.id, &sent.message.id)
            .await?,
        sent.message
    );
    assert_eq!(
        core.read_messages(&alpha.id, &channel.id, 0, 10).await?,
        vec![sent.message.clone()]
    );
    assert!(matches!(
        core.read_message(&outsider.id, &channel.id, &sent.message.id)
            .await,
        Err(CollabError::PermissionDenied { .. })
    ));

    let other = core.create_channel("other", &user.id).await?;
    core.add_member(&other.id, &alpha.id, &user.id).await?;
    assert!(matches!(
        core.read_message(&alpha.id, &other.id, &sent.message.id)
            .await,
        Err(CollabError::NotFound { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn read_messages_tail_returns_exact_count_and_true_tail() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        channel,
        ..
    } = World::create().await?;
    let outsider = core.create_user("tail-outsider", "Tail Outsider").await?;
    let mut sent = Vec::new();
    for index in 0..5 {
        sent.push(
            core.send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: format!("tail-message-{index}"),
                text: format!("tail {index}"),
            })
            .await?
            .message,
        );
    }

    // A small limit still reports the exact total and the true latest page.
    let tail = core.read_messages_tail(&alpha.id, &channel.id, 2).await?;
    assert_eq!(tail.count, 5);
    assert_eq!(tail.messages, sent[3..].to_vec());
    // A limit above the total returns everything, ascending.
    let full = core.read_messages_tail(&alpha.id, &channel.id, 100).await?;
    assert_eq!(full.count, 5);
    assert_eq!(full.messages, sent);
    // An empty target reports zero with no messages.
    let empty = core.create_channel("tail-empty", &user.id).await?;
    core.add_member(&empty.id, &alpha.id, &user.id).await?;
    let empty_tail = core.read_messages_tail(&alpha.id, &empty.id, 10).await?;
    assert_eq!(empty_tail.count, 0);
    assert!(empty_tail.messages.is_empty());
    // Membership and argument validation match the paged read.
    assert!(matches!(
        core.read_messages_tail(&outsider.id, &channel.id, 2).await,
        Err(CollabError::PermissionDenied { .. })
    ));
    assert!(matches!(
        core.read_messages_tail(&alpha.id, &channel.id, 0).await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.read_messages_tail(&alpha.id, &channel.id, 101).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}

#[tokio::test]
async fn read_messages_tail_holds_one_snapshot_under_concurrent_writes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("tail-snapshot.db");
    let reader = CollabCore::open(&path).await?;
    let user = reader.create_user("owner", "Owner").await?;
    let channel = reader.create_channel("busy", &user.id).await?;
    let writer = CollabCore::open(&path).await?;

    let writer_user = writer.ensure_user("owner", "Owner").await?;
    let write_target = channel.id.clone();

    let writer_task = tokio::spawn(async move {
        for index in 0..30 {
            writer
                .send_message(SendMessageRequest {
                    target_id: write_target.clone(),
                    author_id: writer_user.id.clone(),
                    client_request_id: format!("concurrent-{index}"),
                    text: format!("concurrent {index}"),
                })
                .await?;
            tokio::task::yield_now().await;
        }
        Ok::<(), CollabError>(())
    });

    // Every read must be self-consistent: the page is the contiguous
    // suffix implied by the count of the same snapshot.
    for _ in 0..30 {
        let tail = reader.read_messages_tail(&user.id, &channel.id, 5).await?;
        let page_len = i64::try_from(tail.messages.len()).unwrap();
        assert!(page_len <= tail.count);
        if page_len > 0 {
            let first_seq = tail.messages[0].seq;
            assert_eq!(first_seq, tail.count - page_len + 1);
            for (offset, message) in tail.messages.iter().enumerate() {
                assert_eq!(message.seq, first_seq + i64::try_from(offset).unwrap());
            }
        }
        tokio::task::yield_now().await;
    }
    writer_task.await.unwrap()?;
    Ok(())
}
