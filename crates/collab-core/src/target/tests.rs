use crate::test_support::*;

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
