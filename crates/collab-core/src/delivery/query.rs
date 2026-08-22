//! Wake ledger projections.

use crate::{CollabCore, CollabError, PendingWake, Result};

use super::store::DeliveryStore;

impl CollabCore {
    /// Scan the level-triggered wake ledger. Only authorized deliveries that
    /// have not reached a model request contribute to the returned watermark.
    pub async fn list_pending_wakes(&self, limit: u32) -> Result<Vec<PendingWake>> {
        if limit == 0 || limit > 1000 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 1000".into(),
            ));
        }
        self.read(async |connection| DeliveryStore::new(connection).pending_wakes(limit).await)
            .await
    }
}
