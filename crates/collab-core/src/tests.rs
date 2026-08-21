use std::sync::Arc;

use super::*;

async fn fixture() -> Result<(Arc<CollabCore>, Actor, Actor, Actor, Target)> {
    let core = Arc::new(CollabCore::open_memory().await?);
    let user = core.create_user("owner", "Owner").await?;
    let alpha = core.create_agent("alpha", "Alpha", "/tmp/alpha").await?;
    let beta = core.create_agent("beta", "Beta", "/tmp/beta").await?;
    let channel = core.create_channel("design", &user.id).await?;
    core.add_member(&channel.id, &alpha.id, &user.id).await?;
    core.add_member(&channel.id, &beta.id, &user.id).await?;
    Ok((core, user, alpha, beta, channel))
}

#[tokio::test]
async fn send_commits_message_deliveries_and_wakes_together() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
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
    let (core, user, _alpha, _beta, channel) = fixture().await?;
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
    let (core, user, _alpha, _beta, channel) = fixture().await?;
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
async fn runtime_generation_fences_model_seen_receipts() -> Result<()> {
    let (core, user, alpha, _beta, channel) = fixture().await?;
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

#[tokio::test]
async fn wake_watermark_is_level_triggered_and_generation_fenced() -> Result<()> {
    let (core, user, alpha, _beta, channel) = fixture().await?;
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

#[tokio::test]
async fn exact_target_reads_recheck_current_membership() -> Result<()> {
    let (core, user, alpha, _beta, channel) = fixture().await?;
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
    let (core, user, alpha, _beta, channel) = fixture().await?;
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

#[tokio::test]
async fn ensure_user_migrates_display_name_on_stable_handle() -> Result<()> {
    let core = CollabCore::open_memory().await?;
    let created = core.ensure_user("local-user", "Local User").await?;
    let channel = core.create_channel("identity", &created.id).await?;

    // Same handle with a new display name keeps the actor id, so
    // Memberships and Tasks survive the rename.
    let renamed = core.ensure_user("local-user", "Updated User").await?;
    assert_eq!(renamed.id, created.id);
    assert_eq!(renamed.display_name, "Updated User");
    let members = core.list_target_members(&renamed.id, &channel.id).await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].id, created.id);

    // Repeating with the current name is a no-op.
    let stable = core.ensure_user("local-user", "Updated User").await?;
    assert_eq!(stable, renamed);

    // A custom name is never overwritten by a later ensure.
    let custom = core.ensure_user("alice", "Custom Alice").await?;
    let kept = core.ensure_user("alice", "Updated User").await?;
    assert_eq!(kept.id, custom.id);
    assert_eq!(kept.display_name, "Custom Alice");
    Ok(())
}

#[tokio::test]
async fn agent_profile_normalizes_charter_and_fences_updates() -> Result<()> {
    let core = CollabCore::open_memory().await?;
    let owner = core.create_user("owner", "Owner").await?;
    let profile = core
        .create_agent_profile(
            "reviewer",
            "Reviewer",
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
    assert_eq!(updated.actor.handle, "reviewer");
    assert_eq!(updated.actor.display_name, "Frontend Reviewer");
    assert_eq!(updated.workspace_path, profile.workspace_path);
    assert!(matches!(
        core.update_agent_profile(
            &profile.actor.id,
            "Stale",
            AgentCharter::default(),
            profile.version,
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
    let (core, user, alpha, beta, _channel) = fixture().await?;
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
async fn agent_membership_directory_is_top_level_and_viewer_filtered() -> Result<()> {
    let (core, owner, alpha, _beta, channel) = fixture().await?;
    let direct = core.create_direct(&owner.id, &alpha.id).await?;
    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: owner.id.clone(),
            client_request_id: "membership-root".into(),
            text: "Open a thread".into(),
        })
        .await?
        .message;
    let _thread = core.create_thread(&root.id, &owner.id).await?;

    let memberships = core.list_agent_memberships(&owner.id, &alpha.id).await?;
    assert_eq!(memberships.len(), 2);
    assert_eq!(memberships[0].target, channel);
    assert_eq!(memberships[0].role, MembershipRole::Member);
    assert_eq!(memberships[1].target, direct);
    assert_eq!(memberships[1].role, MembershipRole::Member);
    assert!(
        memberships
            .iter()
            .all(|membership| membership.target.kind != TargetKind::Thread)
    );

    let outsider = core.create_user("outsider", "Outsider").await?;
    assert!(
        core.list_agent_memberships(&outsider.id, &alpha.id)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn identity_context_returns_role_bearing_inherited_roster() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
    let channel_context = core.identity_context(&alpha.id, Some(&channel.id)).await?;
    assert_eq!(channel_context.agent.actor.handle, "alpha");
    assert_eq!(channel_context.target.as_ref(), Some(&channel));
    assert_eq!(channel_context.membership_target.as_ref(), Some(&channel));
    assert_eq!(channel_context.members.len(), 3);
    assert!(
        channel_context
            .members
            .iter()
            .any(|member| { member.actor.id == user.id && member.role == MembershipRole::Owner })
    );
    assert!(
        channel_context
            .members
            .iter()
            .any(|member| { member.actor.id == beta.id && member.actor.handle == "beta" })
    );

    let root = core
        .send_message(SendMessageRequest {
            target_id: channel.id.clone(),
            author_id: user.id.clone(),
            client_request_id: "identity-root".into(),
            text: "Discuss in a Thread".into(),
        })
        .await?
        .message;
    let thread = core.create_thread(&root.id, &user.id).await?;
    let thread_context = core.identity_context(&alpha.id, Some(&thread.id)).await?;
    assert_eq!(thread_context.target.as_ref(), Some(&thread));
    assert_eq!(thread_context.membership_target.as_ref(), Some(&channel));
    assert_eq!(thread_context.members, channel_context.members);

    let outsider = core
        .create_agent("outsider-agent", "Outsider", "/tmp/outsider-agent")
        .await?;
    assert!(matches!(
        core.identity_context(&outsider.id, Some(&channel.id)).await,
        Err(CollabError::PermissionDenied { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn list_target_members_returns_only_active_channel_members() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
    let outsider = core.create_user("outsider", "Outsider").await?;

    // The projection is the Channel membership, not the actor directory:
    // the fixture created extra actors that never joined this channel.
    let members = core.list_target_members(&user.id, &channel.id).await?;
    let mut member_ids: Vec<&str> = members.iter().map(|actor| actor.id.as_str()).collect();
    member_ids.sort_unstable();
    let mut expected_ids = vec![alpha.id.as_str(), beta.id.as_str(), user.id.as_str()];
    expected_ids.sort_unstable();
    assert_eq!(member_ids, expected_ids);

    // A second channel has its own membership.
    let other = core.create_channel("other", &user.id).await?;
    let other_members = core.list_target_members(&user.id, &other.id).await?;
    assert_eq!(other_members.len(), 1);
    assert_eq!(other_members[0].id, user.id);

    // Non-members are rejected, and left members disappear.
    assert!(matches!(
        core.list_target_members(&outsider.id, &channel.id).await,
        Err(CollabError::PermissionDenied { .. })
    ));
    Ok(())
}

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
async fn direct_target_is_unique_for_an_unordered_actor_pair() -> Result<()> {
    let (core, user, alpha, beta, _channel) = fixture().await?;
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
async fn thread_inherits_parent_access_and_delivers_only_to_followers() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
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

#[tokio::test]
async fn activity_inbox_projects_done_revive_direct_and_task_metadata() -> Result<()> {
    let (core, user, alpha, beta, channel) = fixture().await?;
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

    let page = core.inbox_list(&user.id, 20, None).await?;
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
    let after_done = core.inbox_list(&user.id, 20, None).await?;
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
    let after_reopen = core.inbox_list(&user.id, 20, None).await?;
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
    let (core, user, _alpha, beta, channel) = fixture().await?;
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
        core.inbox_list(&user.id, 20, None)
            .await?
            .items
            .iter()
            .any(|item| item.conversation_id == thread.id)
    );
    core.unfollow_thread(&thread.id, &user.id).await?;
    assert!(
        core.inbox_list(&user.id, 20, None)
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
        core.inbox_list(&user.id, 20, None)
            .await?
            .items
            .iter()
            .any(|item| item.conversation_id == thread.id)
    );

    assert!(
        core.inbox_list(&outsider.id, 20, None)
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

    let first = core.inbox_list(&user.id, 2, None).await?;
    assert_eq!(first.active_count, 5);
    assert_eq!(first.items.len(), 2);
    let second = core
        .inbox_list(&user.id, 2, first.next_cursor.as_deref())
        .await?;
    assert_eq!(second.items.len(), 2);
    let third = core
        .inbox_list(&user.id, 2, second.next_cursor.as_deref())
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
        core.inbox_list(&user.id, 0, None).await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.inbox_list(&user.id, 20, Some("bad-cursor")).await,
        Err(CollabError::InvalidArgument(_))
    ));
    assert!(matches!(
        core.inbox_done(&user.id, &expected[0], i64::MAX).await,
        Err(CollabError::InvalidArgument(_))
    ));
    Ok(())
}

#[tokio::test]
async fn schema_v4_upgrades_change_ledger_for_activity_done() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v4.db");
    let user_id = "018f0000-0000-7000-8000-000000000001";
    let target_id = "018f0000-0000-7000-8000-000000000002";
    {
        let database = turso::Builder::new_local(
            path.to_str()
                .ok_or_else(|| CollabError::Filesystem("temporary path is not UTF-8".into()))?,
        )
        .build()
        .await?;
        let mut connection = database.connect()?;
        connection.execute_batch(META_SCHEMA).await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        transaction.execute_batch(SCHEMA_V1).await?;
        transaction.execute_batch(SCHEMA_V2).await?;
        transaction.execute_batch(SCHEMA_V3).await?;
        transaction.execute_batch(SCHEMA_V4).await?;
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                     VALUES (?1, 'user', 'legacy-owner', 'Legacy Owner', 1)",
                [user_id],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO targets
                     (id, kind, name, created_by, created_at_ms, archived_at_ms)
                     VALUES (?1, 'channel', 'legacy', ?2, 1, NULL)",
                (target_id, user_id),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, 'owner', 1, NULL)",
                (target_id, user_id),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO messages
                     (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                     VALUES ('018f0000-0000-7000-8000-000000000003', ?1, ?2,
                             'legacy-request', '{\"kind\":\"text\",\"text\":\"legacy\"}', 1)",
                (target_id, user_id),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO change_events
                     (kind, target_id, entity_id, created_at_ms)
                     VALUES ('message_created', ?1,
                             '018f0000-0000-7000-8000-000000000003', 1)",
                [target_id],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO change_recipients (change_seq, actor_id) VALUES (1, ?1)",
                [user_id],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '4')",
                (),
            )
            .await?;
        transaction.commit().await?;
    }

    let core = CollabCore::open(&path).await?;
    let page = core.inbox_list(user_id, 20, None).await?;
    assert_eq!(page.active_count, 1);
    let through_seq = page.items[0].last_activity_seq;
    core.inbox_done(user_id, target_id, through_seq).await?;
    let changes = core.list_changes(user_id, 0, 10).await?;
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
    assert_eq!(changes[1].kind, ChangeKind::ActivityDoneChanged);
    Ok(())
}

#[tokio::test]
async fn schema_v5_upgrades_agents_with_default_charter() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v5.db");
    let agent_id = "018f0000-0000-7000-8000-000000000011";
    {
        let database = turso::Builder::new_local(
            path.to_str()
                .ok_or_else(|| CollabError::Filesystem("temporary path is not UTF-8".into()))?,
        )
        .build()
        .await?;
        let mut connection = database.connect()?;
        connection.execute_batch(META_SCHEMA).await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        transaction.execute_batch(SCHEMA_V1).await?;
        transaction.execute_batch(SCHEMA_V2).await?;
        transaction.execute_batch(SCHEMA_V3).await?;
        transaction.execute_batch(SCHEMA_V4).await?;
        transaction.execute_batch(SCHEMA_V5).await?;
        transaction
            .execute(
                "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                     VALUES (?1, 'agent', 'legacy-agent', 'Legacy Agent', 1)",
                [agent_id],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO agents
                     (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms)
                     VALUES (?1, '/tmp/legacy-agent', 'active', 1, 1)",
                [agent_id],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '5')",
                (),
            )
            .await?;
        transaction.commit().await?;
    }

    let core = CollabCore::open(&path).await?;
    let profile = core.agent_profile(agent_id).await?;
    assert_eq!(profile.version, 1);
    assert_eq!(profile.charter, AgentCharter::default());
    let connection = core.connection.lock().await;
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = 'schema_version'",
            (),
        )
        .await?;
    assert_eq!(
        rows.next().await?.expect("schema row").get::<String>(0)?,
        SCHEMA_VERSION.to_string()
    );
    Ok(())
}

#[tokio::test]
async fn schema_v1_file_upgrades_to_unique_session_bindings() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v1.db");
    {
        let database = turso::Builder::new_local(
            path.to_str()
                .ok_or_else(|| CollabError::Filesystem("temporary path is not UTF-8".into()))?,
        )
        .build()
        .await?;
        let mut connection = database.connect()?;
        connection.execute_batch(META_SCHEMA).await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        transaction.execute_batch(SCHEMA_V1).await?;
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '1')",
                (),
            )
            .await?;
        transaction.commit().await?;
    }

    let core = CollabCore::open(&path).await?;
    let alpha = core
        .create_agent("v1-alpha", "Alpha", "/tmp/v1-alpha")
        .await?;
    let beta = core.create_agent("v1-beta", "Beta", "/tmp/v1-beta").await?;
    core.bind_runtime(&alpha.id, "unique-session", "openai", "codex", "default")
        .await?;
    assert!(
        core.bind_runtime(&beta.id, "unique-session", "openai", "codex", "default")
            .await
            .is_err()
    );
    let connection = core.connection.lock().await;
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = 'schema_version'",
            (),
        )
        .await?;
    assert_eq!(
        rows.next().await?.expect("schema row").get::<String>(0)?,
        SCHEMA_VERSION.to_string()
    );
    Ok(())
}

#[tokio::test]
async fn local_turso_file_reopens_with_agent_identity_intact() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.db");
    let owner_id;
    {
        let core = CollabCore::open(&path).await?;
        owner_id = core.create_user("persistent-owner", "Owner").await?.id;
        core.close().await?;
    }

    let reopened = CollabCore::open(&path).await?;
    let channel = reopened.create_channel("after-restart", &owner_id).await?;
    assert_eq!(channel.created_by, owner_id);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(directory.path())?.permissions().mode() & 0o777,
            0o700
        );
    }
    Ok(())
}

#[tokio::test]
async fn delete_agent_removes_operational_state_but_keeps_history() -> Result<()> {
    let (core, user, alpha, _beta, channel) = fixture().await?;
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
    let (core, user, _alpha, _beta, _channel) = fixture().await?;
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

async fn count(connection: &Connection, table: &str) -> Result<i64> {
    let allowed = [
        "messages",
        "deliveries",
        "agent_wake_state",
        "inbox_batch_items",
        "actors",
    ];
    assert!(allowed.contains(&table));
    let mut rows = connection
        .query(format!("SELECT COUNT(*) FROM {table}"), ())
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
    Ok(row.get(0)?)
}

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
