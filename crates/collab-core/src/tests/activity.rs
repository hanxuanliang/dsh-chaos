use super::test_support::*;
use crate::{
    ActivityTitleKind, ActorKind, ChangeKind, CollabCore, CollabError, Result, SendMessageRequest,
    TaskStatus,
};

#[tokio::test]
async fn activity_inbox_projects_done_revive_direct_and_task_metadata() -> Result<()> {
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
    let task_channel = core.create_channel("tasks", &user.id).await?;
    core.add_member(&task_channel.id, &alpha.id, &user.id)
        .await?;
    let task_message = core
        .send_message(SendMessageRequest {
            target_id: task_channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "activity-task".into(),
            text: "ship the inbox".into(),
        })
        .await?;
    core.create_task(&task_message.message.id, &user.id).await?;
    core.claim_task(&task_message.message.id, &alpha.id).await?;

    let direct = core.create_direct(&user.id, &alpha.id).await?;
    core.send_message(SendMessageRequest {
        target_id: direct.id.clone(),
        author_id: alpha.id.clone(),
        client_request_id: "activity-direct".into(),
        text: "direct update".into(),
    })
    .await?;

    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "activity-root".into(),
            text: "thread root".into(),
        })
        .await?;
    core.create_task(&root.message.id, &user.id).await?;
    core.claim_task(&root.message.id, &alpha.id).await?;
    let thread = core.create_thread(&root.message.id, &beta.id).await?;
    let reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: beta.id.clone(),
            client_request_id: "activity-reply".into(),
            text: "thread reply".into(),
        })
        .await?;

    let page = core.inbox_list(&user.id, 20, None, None).await?;
    assert_eq!(page.active_count, 4);
    assert_eq!(page.items.len(), 4);

    let direct_item = page
        .items
        .iter()
        .find(|item| item.conversation_id == direct.id)
        .expect("Direct Activity item");
    assert_eq!(direct_item.target_name, alpha.display_name);
    assert_eq!(direct_item.title, "direct update");
    assert_eq!(
        direct_item
            .latest_reply
            .as_ref()
            .map(|reply| reply.sender_kind),
        Some(ActorKind::Agent)
    );
    assert_eq!(direct_item.reply_count, None);

    let task_item = page
        .items
        .iter()
        .find(|item| item.conversation_id == task_channel.id)
        .expect("Channel Task Activity item");
    let task_badge = task_item.task.as_ref().expect("Channel Task badge");
    assert_eq!(task_badge.number, 1);
    assert_eq!(task_badge.status, TaskStatus::InProgress);
    assert_eq!(task_badge.assignee_name.as_deref(), Some("Alpha"));

    let thread_item = page
        .items
        .iter()
        .find(|item| item.conversation_id == thread.id)
        .expect("Thread Activity item");
    assert_eq!(
        thread_item.parent_target_id.as_deref(),
        Some(channel.id.as_str())
    );
    assert_eq!(
        thread_item.root_message_id.as_deref(),
        Some(root.message.id.as_str())
    );
    assert_eq!(thread_item.title_kind, ActivityTitleKind::Thread);
    assert_eq!(thread_item.title, "thread root");
    assert_eq!(thread_item.reply_count, Some(1));
    assert_eq!(thread_item.last_activity_seq, reply.message.seq);
    assert_eq!(
        thread_item
            .latest_reply
            .as_ref()
            .map(|preview| preview.excerpt.as_str()),
        Some("thread reply")
    );
    assert_eq!(
        thread_item.task.as_ref().map(|task| task.status),
        Some(TaskStatus::InProgress)
    );

    let cursor_before_done = core.snapshot(&user.id).await?.cursor;
    let alpha_cursor_before_done = core.snapshot(&alpha.id).await?.cursor;
    core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
        .await?;
    let after_done = core.inbox_list(&user.id, 20, None, None).await?;
    assert_eq!(after_done.active_count, 3);
    assert!(
        after_done
            .items
            .iter()
            .all(|item| item.conversation_id != task_channel.id)
    );
    let done_changes = core.list_changes(&user.id, cursor_before_done, 10).await?;
    assert_eq!(done_changes.len(), 1);
    assert_eq!(done_changes[0].kind, ChangeKind::ActivityDoneChanged);
    assert!(
        core.list_changes(&alpha.id, alpha_cursor_before_done, 10)
            .await?
            .is_empty()
    );

    let cursor_after_done = done_changes[0].seq;
    core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
        .await?;
    assert!(
        core.list_changes(&user.id, cursor_after_done, 10)
            .await?
            .is_empty()
    );

    let reopened = core
        .send_message(SendMessageRequest {
            target_id: task_channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "activity-reopen".into(),
            text: "new activity".into(),
        })
        .await?;
    core.inbox_done(&user.id, &task_channel.id, task_message.message.seq)
        .await?;
    let after_reopen = core.inbox_list(&user.id, 20, None, None).await?;
    assert_eq!(after_reopen.active_count, 4);
    assert_eq!(
        after_reopen
            .items
            .iter()
            .find(|item| item.conversation_id == task_channel.id)
            .map(|item| item.last_activity_seq),
        Some(reopened.message.seq)
    );
    Ok(())
}

#[tokio::test]
async fn activity_inbox_enforces_membership_and_thread_follow_scope() -> Result<()> {
    let World {
        core,
        user,
        beta,
        channel,
        ..
    } = World::create().await?;
    let outsider = core.create_user("outsider", "Outsider").await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "follow-root".into(),
            text: "follow scope".into(),
        })
        .await?;
    let thread = core.create_thread(&root.message.id, &beta.id).await?;
    let reply = core
        .send_message(SendMessageRequest {
            target_id: thread.id.clone(),
            author_id: beta.id.clone(),
            client_request_id: "follow-reply".into(),
            text: "followed reply".into(),
        })
        .await?;

    assert!(
        core.inbox_list(&user.id, 20, None, None)
            .await?
            .items
            .iter()
            .any(|item| item.conversation_id == thread.id)
    );
    core.unfollow_thread(&thread.id, &user.id).await?;
    assert!(
        core.inbox_list(&user.id, 20, None, None)
            .await?
            .items
            .iter()
            .all(|item| item.conversation_id != thread.id)
    );
    assert!(matches!(
        core.inbox_done(&user.id, &thread.id, reply.message.seq)
            .await,
        Err(CollabError::NotFound { .. })
    ));
    core.follow_thread(&thread.id, &user.id).await?;
    assert!(
        core.inbox_list(&user.id, 20, None, None)
            .await?
            .items
            .iter()
            .any(|item| item.conversation_id == thread.id)
    );

    assert!(
        core.inbox_list(&outsider.id, 20, None, None)
            .await?
            .items
            .is_empty()
    );
    assert!(matches!(
        core.inbox_done(&outsider.id, &channel.id, root.message.seq)
            .await,
        Err(CollabError::PermissionDenied { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn activity_inbox_cursor_pages_without_duplicates_or_skips() -> Result<()> {
    let core = CollabCore::open_memory().await?;
    let user = core.create_user("pager", "Pager").await?;
    let mut expected = Vec::new();
    for index in 0..5 {
        let channel = core
            .create_channel(&format!("page-{index}"), &user.id)
            .await?;
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: format!("page-message-{index}"),
            text: format!("activity {index}"),
        })
        .await?;
        expected.push(channel.id);
    }
    expected.reverse();

    let first = core.inbox_list(&user.id, 2, None, None).await?;
    assert_eq!(first.active_count, 5);
    assert_eq!(first.items.len(), 2);
    let second = core
        .inbox_list(&user.id, 2, first.next_cursor.as_deref(), None)
        .await?;
    assert_eq!(second.items.len(), 2);
    let third = core
        .inbox_list(&user.id, 2, second.next_cursor.as_deref(), None)
        .await?;
    assert_eq!(third.items.len(), 1);
    assert!(third.next_cursor.is_none());
    let actual = first
        .items
        .iter()
        .chain(&second.items)
        .chain(&third.items)
        .map(|item| item.conversation_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    let mut unique = actual.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), actual.len());

    assert!(matches!(
        core.inbox_list(&user.id, 0, None, None).await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.inbox_list(&user.id, 20, Some("bad-cursor"), None)
            .await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.inbox_done(&user.id, &expected[0], i64::MAX).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}

#[tokio::test]
async fn inbox_filters_all_unread_and_mark_all_done() -> Result<()> {
    let World {
        core, user, alpha, ..
    } = World::create().await?;
    let channel = core.create_channel("f-all", &user.id).await?;
    for index in 0..3 {
        core.send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: format!("fa-{index}"),
            text: format!("msg {index}"),
        })
        .await?;
    }
    let direct = core.create_direct(&user.id, &alpha.id).await?;
    core.send_message(SendMessageRequest {
        target_id: direct.id.clone(),
        author_id: alpha.id.clone(),
        client_request_id: "fa-d".into(),
        text: "direct msg".into(),
    })
    .await?;

    // Unread default: both conversations visible, none marked done.
    let unread = core.inbox_list(&user.id, 20, None, None).await?;
    assert_eq!(unread.items.len(), 2);
    assert!(unread.items.iter().all(|item| !item.done));
    assert_eq!(unread.active_count, 2);

    // All filter: same set (nothing done yet), still no done flags.
    let all = core.inbox_list(&user.id, 20, None, Some("all")).await?;
    assert_eq!(all.items.len(), 2);
    assert!(all.items.iter().all(|item| !item.done));
    assert_eq!(all.active_count, 2);

    // Mark all: fences advance, unread drains, All shows done flags.
    let advanced = core.inbox_done_all(&user.id).await?;
    assert_eq!(advanced, 2);
    let unread_after = core.inbox_list(&user.id, 20, None, None).await?;
    assert!(unread_after.items.is_empty());
    assert_eq!(unread_after.active_count, 0);
    let all_after = core.inbox_list(&user.id, 20, None, Some("all")).await?;
    assert_eq!(all_after.items.len(), 2);
    assert!(all_after.items.iter().all(|item| item.done));

    // Repeat call is a no-op returning zero.
    let repeat = core.inbox_done_all(&user.id).await?;
    assert_eq!(repeat, 0);

    // New activity revives only that conversation, with done now false.
    core.send_message(SendMessageRequest {
        target_id: channel.id.clone(),
        author_id: user.id.clone(),
        client_request_id: "fa-revive".into(),
        text: "revive".into(),
    })
    .await?;
    let revived = core.inbox_list(&user.id, 20, None, None).await?;
    assert_eq!(revived.items.len(), 1);
    assert_eq!(revived.items[0].conversation_id, channel.id);
    assert!(!revived.items[0].done);

    // Unknown filter text is rejected at the boundary.
    assert!(matches!(
        core.inbox_list(&user.id, 20, None, Some("bogus")).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}
