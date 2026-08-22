use turso::{Connection, Row};

use crate::actor::parse_actor_kind;
use crate::db::{FromRow, QueryRows, require_scalar_row};
use crate::message::stored_text;
use crate::target::parse_target_kind;
use crate::{
    ActivityInboxItem, ActivityInboxReply, ActivityInboxTask, ActivityTitleKind, CollabError,
    Result, TargetKind, TaskStatus,
};

use super::model::ActivityCursor;

const ACTIVITY_INBOX_CANDIDATES_CTE: &str = r#"
WITH eligible AS (
  SELECT target.id,
         target.kind,
         target.name,
         target.parent_target_id,
         target.root_message_id,
         COALESCE(done.done_through_seq, 0) AS done_through_seq
  FROM targets AS target
  JOIN memberships AS membership
    ON membership.target_id = CASE
      WHEN target.kind = 'thread' THEN target.parent_target_id
      ELSE target.id
    END
   AND membership.actor_id = ?1
   AND membership.left_at_ms IS NULL
  LEFT JOIN targets AS parent ON parent.id = target.parent_target_id
  LEFT JOIN activity_inbox_done AS done
    ON done.actor_id = ?1 AND done.target_id = target.id
  WHERE target.archived_at_ms IS NULL
    AND target.kind IN ('channel', 'direct', 'thread')
    AND (
      target.kind <> 'thread'
      OR (
        parent.kind IN ('channel', 'direct')
        AND parent.archived_at_ms IS NULL
        AND EXISTS (
          SELECT 1
          FROM thread_follows AS follow
          WHERE follow.thread_target_id = target.id
            AND follow.actor_id = ?1
            AND follow.unfollowed_at_ms IS NULL
        )
      )
    )
), latest_activity AS (
  SELECT target_id, MAX(seq) AS last_activity_seq
  FROM messages
  GROUP BY target_id
), active AS (
  SELECT eligible.id,
         eligible.kind,
         eligible.name,
         eligible.parent_target_id,
         eligible.root_message_id,
         latest_activity.last_activity_seq
  FROM eligible
  JOIN latest_activity ON latest_activity.target_id = eligible.id
  WHERE latest_activity.last_activity_seq > eligible.done_through_seq
)
"#;

const ACTIVITY_INBOX_ITEMS_SELECT: &str = r#"
SELECT active.id,
       active.kind,
       active.parent_target_id,
       active.root_message_id,
       active.last_activity_seq,
       CASE active.kind
         WHEN 'channel' THEN active.name
         WHEN 'direct' THEN COALESCE(direct_peer.display_name, direct_peer.handle, active.name)
         ELSE CASE parent.kind
           WHEN 'channel' THEN parent.name
           ELSE COALESCE(parent_direct_peer.display_name, parent_direct_peer.handle, parent.name)
         END
       END AS target_name,
       CASE active.kind
         WHEN 'thread' THEN root.body_json
         ELSE latest.body_json
       END AS title_body_json,
       latest_author.kind AS latest_author_kind,
       latest_author.display_name AS latest_author_name,
       latest.body_json AS latest_body_json,
       latest.created_at_ms AS last_activity_at_ms,
       CASE active.kind
         WHEN 'thread' THEN (
           SELECT COUNT(*) FROM messages AS reply WHERE reply.target_id = active.id
         )
         ELSE NULL
       END AS reply_count,
       task.number AS task_number,
       task.status AS task_status,
       assignee.display_name AS task_assignee_name
FROM active
JOIN messages AS latest
  ON latest.target_id = active.id AND latest.seq = active.last_activity_seq
JOIN actors AS latest_author ON latest_author.id = latest.author_id
LEFT JOIN messages AS root ON root.id = active.root_message_id
LEFT JOIN targets AS parent ON parent.id = active.parent_target_id
LEFT JOIN direct_pairs AS direct_pair ON direct_pair.target_id = active.id
LEFT JOIN actors AS direct_peer ON direct_peer.id = CASE
  WHEN direct_pair.actor_low_id = ?1 THEN direct_pair.actor_high_id
  WHEN direct_pair.actor_high_id = ?1 THEN direct_pair.actor_low_id
  ELSE NULL
END
LEFT JOIN direct_pairs AS parent_direct_pair ON parent_direct_pair.target_id = parent.id
LEFT JOIN actors AS parent_direct_peer ON parent_direct_peer.id = CASE
  WHEN parent_direct_pair.actor_low_id = ?1 THEN parent_direct_pair.actor_high_id
  WHEN parent_direct_pair.actor_high_id = ?1 THEN parent_direct_pair.actor_low_id
  ELSE NULL
END
LEFT JOIN tasks AS task ON task.message_id = CASE
  WHEN active.kind = 'thread' THEN active.root_message_id
  ELSE latest.id
END
LEFT JOIN actors AS assignee ON assignee.id = task.assignee_id
"#;

impl FromRow for ActivityInboxItem {
    fn from_row(row: &Row) -> Result<Self> {
        let conversation_id = row.get::<String>(0)?;
        let target_kind_text = row.get::<String>(1)?;
        let target_kind = parse_target_kind(&conversation_id, &target_kind_text)?;
        let parent_target_id = row.get::<Option<String>>(2)?;
        let root_message_id = row.get::<Option<String>>(3)?;
        match target_kind {
            TargetKind::Thread if parent_target_id.is_none() || root_message_id.is_none() => {
                return Err(CollabError::Database(format!(
                    "Activity Thread '{conversation_id}' is missing its parent or root Message"
                )));
            }
            TargetKind::Channel | TargetKind::Direct
                if parent_target_id.is_some() || root_message_id.is_some() =>
            {
                return Err(CollabError::Database(format!(
                    "non-Thread Activity target '{conversation_id}' has Thread metadata"
                )));
            }
            _ => {}
        }
        let last_activity_seq = row.get::<i64>(4)?;
        if last_activity_seq <= 0 {
            return Err(CollabError::Database(format!(
                "Activity target '{conversation_id}' has invalid sequence {last_activity_seq}"
            )));
        }
        let title_body_json = row.get::<Option<String>>(6)?.ok_or_else(|| {
            CollabError::Database(format!(
                "Activity target '{conversation_id}' has no title Message"
            ))
        })?;
        let latest_kind_text = row.get::<String>(7)?;
        let latest_body_json = row.get::<String>(9)?;
        let reply_count = row.get::<Option<i64>>(11)?;
        if reply_count.is_some_and(|count| count < 0) {
            return Err(CollabError::Database(format!(
                "Activity target '{conversation_id}' has a negative reply count"
            )));
        }
        let task_number = row.get::<Option<i64>>(12)?;
        let task_status = row.get::<Option<String>>(13)?;
        let task = match (task_number, task_status) {
            (None, None) => None,
            (Some(number), Some(status_text)) if number > 0 => {
                let status = TaskStatus::parse(&status_text).ok_or_else(|| {
                    CollabError::Database(format!(
                        "Activity target '{conversation_id}' has invalid Task status '{status_text}'"
                    ))
                })?;
                Some(ActivityInboxTask {
                    number,
                    status,
                    assignee_name: row.get(14)?,
                })
            }
            _ => {
                return Err(CollabError::Database(format!(
                    "Activity target '{conversation_id}' has partial Task metadata"
                )));
            }
        };
        Ok(Self {
            conversation_id: conversation_id.clone(),
            target_kind,
            parent_target_id,
            root_message_id,
            target_name: row.get(5)?,
            title_kind: if target_kind == TargetKind::Thread {
                ActivityTitleKind::Thread
            } else {
                ActivityTitleKind::Message
            },
            title: stored_text(
                &title_body_json,
                &format!("Activity title for '{conversation_id}'"),
            )?,
            latest_reply: Some(ActivityInboxReply {
                sender_name: row.get(8)?,
                sender_kind: parse_actor_kind(&conversation_id, &latest_kind_text)?,
                excerpt: stored_text(
                    &latest_body_json,
                    &format!("Activity preview for '{conversation_id}'"),
                )?,
                at_ms: row.get(10)?,
            }),
            last_activity_at_ms: row.get(10)?,
            last_activity_seq,
            reply_count,
            task,
        })
    }
}

struct CountRow(i64);

impl FromRow for CountRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

struct MaxSeqRow(Option<i64>);

impl FromRow for MaxSeqRow {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self(row.get(0)?))
    }
}

pub(crate) struct ActivityStore<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ActivityStore<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn active_count(&self, actor_id: &str) -> Result<i64> {
        let row = self
            .connection
            .query_row::<CountRow>(
                &format!("{ACTIVITY_INBOX_CANDIDATES_CTE} SELECT COUNT(*) FROM active"),
                [actor_id],
            )
            .await?;
        Ok(require_scalar_row(row, "Activity count")?.0)
    }

    /// Read one page after `cursor`, fetching `limit + 1` rows so the caller
    /// can detect a following page without a second query.
    pub(crate) async fn inbox_page(
        &self,
        actor_id: &str,
        cursor: Option<&ActivityCursor>,
        limit: u32,
    ) -> Result<Vec<ActivityInboxItem>> {
        let cursor_seq = cursor.map_or(0, |cursor| cursor.last_activity_seq);
        let cursor_target_id = cursor.map_or("", |cursor| cursor.conversation_id.as_str());
        self.connection
            .query_rows(
                &format!(
                    "{ACTIVITY_INBOX_CANDIDATES_CTE}{ACTIVITY_INBOX_ITEMS_SELECT}
                     WHERE (?2 = 0)
                        OR active.last_activity_seq < ?2
                        OR (active.last_activity_seq = ?2 AND active.id > ?3)
                     ORDER BY active.last_activity_seq DESC, active.id ASC
                     LIMIT ?4"
                ),
                (actor_id, cursor_seq, cursor_target_id, i64::from(limit) + 1),
            )
            .await
    }

    /// The newest Message sequence of one target, `None` when it has none.
    pub(crate) async fn latest_seq(&self, target_id: &str) -> Result<Option<i64>> {
        let row = self
            .connection
            .query_row::<MaxSeqRow>(
                "SELECT MAX(seq) FROM messages WHERE target_id = ?1",
                [target_id],
            )
            .await?;
        Ok(require_scalar_row(row, "latest Activity query")?.0)
    }

    /// Advance the actor's Done fence; returns `true` when the fence moved.
    pub(crate) async fn set_done_fence(
        &self,
        actor_id: &str,
        target_id: &str,
        through_seq: i64,
        now: i64,
    ) -> Result<bool> {
        let changed = self
            .connection
            .execute(
                "INSERT INTO activity_inbox_done
                 (actor_id, target_id, done_through_seq, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(actor_id, target_id) DO UPDATE SET
                   done_through_seq = excluded.done_through_seq,
                   updated_at_ms = excluded.updated_at_ms
                 WHERE excluded.done_through_seq > activity_inbox_done.done_through_seq",
                (actor_id, target_id, through_seq, now),
            )
            .await?;
        Ok(changed == 1)
    }
}
