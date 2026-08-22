//! Process-local core handle and shared value helpers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;
use turso::Connection;
use turso::transaction::TransactionBehavior;
use uuid::Uuid;

use crate::{CollabError, Result};

/// One process-local handle over the authoritative local Turso database.
pub struct CollabCore {
    pub(crate) connection: Mutex<Connection>,
    pub(crate) closed: AtomicBool,
}

impl CollabCore {
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
