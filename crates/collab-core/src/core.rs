//! Process-local core handle, lifecycle, and shared value helpers.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;
use turso::transaction::TransactionBehavior;
use turso::Connection;
use uuid::Uuid;

use crate::db::migrate::migrate;
use crate::{CollabError, Result};

/// One process-local handle over the authoritative local Turso database.
pub struct CollabCore {
    pub(crate) connection: Mutex<Connection>,
    closed: AtomicBool,
}

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

    /// Stop admitting new operations. In-flight operations hold the connection
    /// mutex and finish before this future returns.
    pub async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        let _connection = self.connection.lock().await;
        Ok(())
    }

    pub(crate) fn assert_open(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            Err(CollabError::Closed)
        } else {
            Ok(())
        }
    }

    pub(crate) async fn read<T, F>(&self, f: F) -> Result<T>
    where
        F: for<'connection> AsyncFnOnce(&'connection Connection) -> Result<T>,
    {
        self.assert_open()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        let value = f(&transaction).await?;
        transaction.commit().await?;
        Ok(value)
    }
}

pub(crate) fn require_non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CollabError::InvalidArgument(format!(
            "{name} must not be blank"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn not_found(entity: &'static str, id: &str) -> CollabError {
    CollabError::NotFound {
        entity,
        id: id.to_owned(),
    }
}

pub(crate) fn new_id() -> String {
    Uuid::now_v7().to_string()
}

pub(crate) fn now_ms() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CollabError::Filesystem(error.to_string()))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| CollabError::Filesystem("system clock is outside i64 milliseconds".into()))
}

pub(crate) fn prepare_database_parent(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    protect_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

pub(crate) fn protect_database_file(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") || !path.exists() {
        return Ok(());
    }
    protect_file(path)
}

#[cfg(unix)]
pub(crate) fn protect_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn protect_file(_path: &Path) -> Result<()> {
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
