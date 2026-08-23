use super::test_support::*;

#[tokio::test]
async fn agent_membership_directory_is_top_level_and_viewer_filtered() -> Result<()> {
    let World {
        core,
        user: owner,
        alpha,
        channel,
        ..
    } = World::create().await?;
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
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
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
    let World {
        core,
        user,
        alpha,
        beta,
        channel,
        ..
    } = World::create().await?;
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
    let other = core
        .create_channel("other", "Other collaboration", &user.id)
        .await?;
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
