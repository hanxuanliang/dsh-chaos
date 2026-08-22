//! Change retention mutation.

use crate::{CollabCore, CollabError, Result};

use super::store::{change_retention_floor, latest_change_seq, set_retention_floor};

impl CollabCore {
    /// Delete the oldest contiguous prefix of change events older than one
    /// wall-clock cutoff and persist the newest cursor that remains safe for
    /// incremental replay. A non-monotonic clock may retain extra old rows,
    /// but can never create a replay hole.
    pub async fn prune_changes_before(&self, before_ms: i64) -> Result<i64> {
        if before_ms < 0 {
            return Err(CollabError::InvalidArgument(
                "before_ms must not be negative".into(),
            ));
        }
        self.write(async |connection| {
            let current_floor = change_retention_floor(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT MIN(seq) FROM change_events WHERE created_at_ms >= ?1",
                    [before_ms],
                )
                .await?;
            let Some(row) = rows.next().await? else {
                return Err(CollabError::Database(
                    "change retention query returned no row".into(),
                ));
            };
            let first_retained_seq = row.get::<Option<i64>>(0)?;
            drop(rows);
            let prune_through = match first_retained_seq {
                Some(first_retained_seq) => first_retained_seq - 1,
                None => latest_change_seq(connection).await?,
            };
            if prune_through <= current_floor {
                return Ok(current_floor);
            }
            connection
                .execute(
                    "DELETE FROM change_recipients WHERE change_seq <= ?1",
                    [prune_through],
                )
                .await?;
            connection
                .execute("DELETE FROM change_events WHERE seq <= ?1", [prune_through])
                .await?;
            set_retention_floor(connection, prune_through).await?;
            Ok(prune_through)
        })
        .await
    }
}
