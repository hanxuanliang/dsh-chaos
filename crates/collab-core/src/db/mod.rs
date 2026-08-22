//! Turso-backed persistence, connection lifecycle, and immutable schema migrations.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use tokio::sync::Mutex;

use crate::{CollabCore, CollabError, Result};

pub(crate) mod migrate;
pub(crate) mod row;

pub(crate) use row::{FromRow, QueryRows, placeholders};

use migrate::migrate;

impl CollabCore {
    /// Open a local Turso file and apply the current schema.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        prepare_database_parent(path)?;
        let database_path = path
            .to_str()
            .ok_or_else(|| CollabError::InvalidArgument("database path must be UTF-8".into()))?;
        let database = turso::Builder::new_local(database_path).build().await?;
        let mut connection = database.connect()?;
        migrate(&mut connection).await?;
        protect_database_file(path)?;
        Ok(Self {
            connection: Mutex::new(connection),
            closed: AtomicBool::new(false),
        })
    }

    /// Open an in-memory Turso database for tests and ephemeral hosts.
    pub async fn open_memory() -> Result<Self> {
        Self::open(Path::new(":memory:")).await
    }
}

fn prepare_database_parent(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    protect_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn protect_database_file(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") || !path.exists() {
        return Ok(());
    }
    protect_file(path)
}

#[cfg(unix)]
fn protect_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
