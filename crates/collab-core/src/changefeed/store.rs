//! Change event persistence shared by every other vertical.

use turso::Connection;

use crate::db::QueryRows;
use crate::membership::target_change_recipients;
use crate::{ChangeKind, CollabError, Result};

use super::model::ChangeEvent;

/// The collab_meta key holding the durable change replay floor.
const CHANGE_RETENTION_FLOOR_KEY: &str = "change_retention_floor";

/// The newest durable change cursor, never below the retention floor.
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

/// The oldest cursor still safe for incremental replay.
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
    drop(rows);
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

/// Persist the newest cursor that remains safe for incremental replay.
pub(crate) async fn set_retention_floor(connection: &Connection, floor: i64) -> Result<()> {
    connection
        .execute(
            "INSERT INTO collab_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (CHANGE_RETENTION_FLOOR_KEY, floor.to_string()),
        )
        .await?;
    Ok(())
}

/// Changes addressed to one actor after a global cursor, in sequence order.
pub(crate) async fn changes_after(
    connection: &Connection,
    actor_id: &str,
    after_seq: i64,
    limit: u32,
) -> Result<Vec<ChangeEvent>> {
    connection
        .query_rows::<ChangeEvent>(
            "SELECT change.seq, change.kind, change.target_id,
                    change.entity_id, change.created_at_ms
             FROM change_recipients recipient
             JOIN change_events change ON change.seq = recipient.change_seq
             WHERE recipient.actor_id = ?1 AND change.seq > ?2
             ORDER BY change.seq
             LIMIT ?3",
            (actor_id, after_seq, i64::from(limit)),
        )
        .await
}

/// Every durable actor id, in id order, for broadcast changes.
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

/// Append one change event addressed to an explicit recipient snapshot and
/// return its global sequence.
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

/// Append one change event addressed to a target's current membership plus
/// explicit extra actors.
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
