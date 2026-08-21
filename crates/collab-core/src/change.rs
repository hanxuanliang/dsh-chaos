//! Authorization-filtered snapshots and durable change-ledger retention.

use super::*;

impl CollabCore {
    /// Return one authorization-filtered bootstrap projection and the global
    /// durable change cursor observed in the same connection critical section.
    pub async fn snapshot(&self, actor_id: &str) -> Result<CollabSnapshot> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        let actor = find_actor(&connection, actor_id).await?;
        let cursor = latest_change_seq(&connection).await?;
        let targets = targets_for_actor(&connection, actor_id).await?;
        let followed_thread_ids = followed_thread_ids_for_actor(&connection, actor_id).await?;
        let tasks = tasks_for_actor(&connection, actor_id, None).await?;
        Ok(CollabSnapshot {
            actor,
            cursor,
            targets,
            followed_thread_ids,
            tasks,
        })
    }

    /// Return durable changes addressed to one actor after a global cursor.
    pub async fn list_changes(
        &self,
        actor_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ChangeEvent>> {
        self.assert_open()?;
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 500 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 500".into(),
            ));
        }
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        let minimum_cursor = change_retention_floor(&connection).await?;
        let maximum_cursor = latest_change_seq(&connection).await?;
        if after_seq < minimum_cursor || after_seq > maximum_cursor {
            return Err(CollabError::ChangeCursorOutOfRange {
                after_seq,
                minimum_cursor,
                maximum_cursor,
            });
        }
        let mut rows = connection
            .query(
                "SELECT change.seq, change.kind, change.target_id,
                        change.entity_id, change.created_at_ms
                 FROM change_recipients recipient
                 JOIN change_events change ON change.seq = recipient.change_seq
                 WHERE recipient.actor_id = ?1 AND change.seq > ?2
                 ORDER BY change.seq
                 LIMIT ?3",
                (actor_id, after_seq, i64::from(limit)),
            )
            .await?;
        let mut changes = Vec::new();
        while let Some(row) = rows.next().await? {
            changes.push(change_from_row(&row)?);
        }
        Ok(changes)
    }

    /// Delete the oldest contiguous prefix of change events older than one
    /// wall-clock cutoff and persist the newest cursor that remains safe for
    /// incremental replay. A non-monotonic clock may retain extra old rows,
    /// but can never create a replay hole.
    pub async fn prune_changes_before(&self, before_ms: i64) -> Result<i64> {
        self.assert_open()?;
        if before_ms < 0 {
            return Err(CollabError::InvalidArgument(
                "before_ms must not be negative".into(),
            ));
        }
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let current_floor = change_retention_floor(&transaction).await?;
        let mut rows = transaction
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
            None => latest_change_seq(&transaction).await?,
        };
        if prune_through <= current_floor {
            transaction.commit().await?;
            return Ok(current_floor);
        }

        transaction
            .execute(
                "DELETE FROM change_recipients WHERE change_seq <= ?1",
                [prune_through],
            )
            .await?;
        transaction
            .execute("DELETE FROM change_events WHERE seq <= ?1", [prune_through])
            .await?;
        transaction
            .execute(
                "INSERT INTO collab_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (CHANGE_RETENTION_FLOOR_KEY, prune_through.to_string()),
            )
            .await?;
        transaction.commit().await?;
        Ok(prune_through)
    }
}

pub(crate) const CHANGE_RETENTION_FLOOR_KEY: &str = "change_retention_floor";

pub(crate) async fn latest_change_seq(connection: &Connection) -> Result<i64> {
    let mut rows = connection
        .query("SELECT COALESCE(MAX(seq), 0) FROM change_events", ())
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::Database(
            "change sequence query returned no row".into(),
        ));
    };
    let latest = row.get::<i64>(0)?;
    drop(rows);
    Ok(latest.max(change_retention_floor(connection).await?))
}

pub(crate) async fn change_retention_floor(connection: &Connection) -> Result<i64> {
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = ?1",
            [CHANGE_RETENTION_FLOOR_KEY],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    let value = row.get::<String>(0)?;
    let floor = value.parse::<i64>().map_err(|_| {
        CollabError::Database(format!(
            "change retention floor '{value}' is not a signed 64-bit integer"
        ))
    })?;
    if floor < 0 {
        return Err(CollabError::Database(format!(
            "change retention floor '{floor}' is negative"
        )));
    }
    Ok(floor)
}

pub(crate) fn change_from_row(row: &Row) -> Result<ChangeEvent> {
    let seq = row.get::<i64>(0)?;
    let kind_text = row.get::<String>(1)?;
    let Some(kind) = ChangeKind::parse(&kind_text) else {
        return Err(CollabError::Database(format!(
            "change '{seq}' has unknown kind '{kind_text}'"
        )));
    };
    Ok(ChangeEvent {
        seq,
        kind,
        target_id: row.get(2)?,
        entity_id: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

pub(crate) async fn all_actor_ids(connection: &Connection) -> Result<Vec<String>> {
    let mut rows = connection
        .query("SELECT id FROM actors ORDER BY id", ())
        .await?;
    let mut actor_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        actor_ids.push(row.get(0)?);
    }
    Ok(actor_ids)
}

pub(crate) async fn insert_change(
    connection: &Connection,
    kind: ChangeKind,
    target_id: Option<&str>,
    entity_id: &str,
    recipient_ids: &[String],
    now: i64,
) -> Result<i64> {
    let mut rows = connection
        .query(
            "INSERT INTO change_events
             (kind, target_id, entity_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             RETURNING seq",
            (kind.as_str(), target_id, entity_id, now),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(CollabError::Database(
            "change insert returned no sequence".into(),
        ));
    };
    let seq = row.get::<i64>(0)?;
    drop(rows);
    for actor_id in recipient_ids {
        connection
            .execute(
                "INSERT INTO change_recipients (change_seq, actor_id)
                 VALUES (?1, ?2)",
                (seq, actor_id.as_str()),
            )
            .await?;
    }
    Ok(seq)
}

pub(crate) async fn insert_target_change(
    connection: &Connection,
    kind: ChangeKind,
    target_id: &str,
    entity_id: &str,
    extra_actor_ids: &[&str],
    now: i64,
) -> Result<i64> {
    let recipients = target_change_recipients(connection, target_id, extra_actor_ids).await?;
    insert_change(
        connection,
        kind,
        Some(target_id),
        entity_id,
        &recipients,
        now,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

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
}
