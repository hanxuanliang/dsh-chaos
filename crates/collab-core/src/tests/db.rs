use crate::{CollabCore, Result};

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
