//! Turso-backed persistence, connection lifecycle, and immutable schema migrations.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Mutex;
use turso::Connection;
use turso::transaction::TransactionBehavior;

use crate::{CollabError, Result};

pub(crate) mod migrate;
pub(crate) mod row;

pub(crate) use row::{FromRow, QueryRows, placeholders};

use migrate::migrate;

/// One process-local handle over the authoritative local Turso database.
pub struct CollabCore {
    pub(crate) connection: Mutex<Connection>,
    pub(crate) closed: AtomicBool,
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

    pub(crate) async fn write<T, F>(&self, f: F) -> Result<T>
    where
        F: for<'connection> AsyncFnOnce(&'connection Connection) -> Result<T>,
    {
        self.assert_open()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let value = f(&transaction).await?;
        transaction.commit().await?;
        Ok(value)
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
