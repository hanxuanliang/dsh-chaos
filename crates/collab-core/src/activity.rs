//! Actor-scoped Activity inbox projection and Done fences.

use super::*;

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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivityCursor {
    last_activity_seq: i64,
    conversation_id: String,
}

impl CollabCore {
    /// List active Activity conversations newest-first. Eligibility and Done
    /// are actor-specific; count and page are read from one database snapshot.
    pub async fn inbox_list(
        &self,
        actor_id: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ActivityInboxPage> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        if limit == 0 || limit > 50 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 50".into(),
            ));
        }
        let cursor = cursor.map(parse_activity_cursor).transpose()?;
        let cursor_seq = cursor.as_ref().map_or(0, |cursor| cursor.last_activity_seq);
        let cursor_target_id = cursor
            .as_ref()
            .map_or("", |cursor| cursor.conversation_id.as_str());

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        require_actor(&transaction, actor_id).await?;

        let count_statement =
            format!("{ACTIVITY_INBOX_CANDIDATES_CTE} SELECT COUNT(*) FROM active");
        let mut count_rows = transaction.query(count_statement, [actor_id]).await?;
        let count_row = count_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("Activity count returned no row".into()))?;
        let active_count = count_row.get::<i64>(0)?;
        drop(count_rows);

        let page_statement = format!(
            "{ACTIVITY_INBOX_CANDIDATES_CTE}{ACTIVITY_INBOX_ITEMS_SELECT}
             WHERE (?2 = 0)
                OR active.last_activity_seq < ?2
                OR (active.last_activity_seq = ?2 AND active.id > ?3)
             ORDER BY active.last_activity_seq DESC, active.id ASC
             LIMIT ?4"
        );
        let mut rows = transaction
            .query(
                page_statement,
                (actor_id, cursor_seq, cursor_target_id, i64::from(limit) + 1),
            )
            .await?;
        let mut items = Vec::with_capacity(limit as usize + 1);
        while let Some(row) = rows.next().await? {
            items.push(activity_inbox_item_from_row(&row)?);
        }
        drop(rows);
        transaction.commit().await?;

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_cursor = has_more
            .then(|| items.last().map(activity_cursor_for_item))
            .flatten();
        Ok(ActivityInboxPage {
            items,
            next_cursor,
            active_count,
        })
    }

    /// Record an actor's Activity disposition fence. The fence only advances;
    /// a newer Message sequence automatically makes the conversation active.
    pub async fn inbox_done(
        &self,
        actor_id: &str,
        target_id: &str,
        through_seq: i64,
    ) -> Result<()> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        require_non_empty("target_id", target_id)?;
        if through_seq <= 0 {
            return Err(CollabError::InvalidArgument(
                "through_seq must be a positive integer".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route =
            require_target_access(&transaction, target_id, actor_id, "mark Activity Done in")
                .await?;
        if route.kind == TargetKind::Thread
            && !is_following_thread(&transaction, target_id, actor_id).await?
        {
            return Err(not_found("active Thread follow", target_id));
        }

        let mut latest_rows = transaction
            .query(
                "SELECT MAX(seq) FROM messages WHERE target_id = ?1",
                [target_id],
            )
            .await?;
        let latest_seq = latest_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("latest Activity query returned no row".into()))?
            .get::<Option<i64>>(0)?
            .ok_or_else(|| not_found("target activity", target_id))?;
        drop(latest_rows);
        if through_seq > latest_seq {
            return Err(CollabError::InvalidArgument(format!(
                "through_seq {through_seq} is newer than target activity {latest_seq}"
            )));
        }

        let changed = transaction
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
        if changed == 1 {
            insert_change(
                &transaction,
                ChangeKind::ActivityDoneChanged,
                Some(target_id),
                target_id,
                &[actor_id.to_owned()],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

fn activity_inbox_item_from_row(row: &Row) -> Result<ActivityInboxItem> {
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
    Ok(ActivityInboxItem {
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

fn parse_activity_cursor(value: &str) -> Result<ActivityCursor> {
    let Some((sequence, conversation_id)) = value.split_once(':') else {
        return Err(CollabError::InvalidArgument(
            "cursor must be '<sequence>:<target-id>'".into(),
        ));
    };
    let last_activity_seq = sequence.parse::<i64>().map_err(|_| {
        CollabError::InvalidArgument("cursor sequence must be a positive integer".into())
    })?;
    if last_activity_seq <= 0
        || conversation_id.is_empty()
        || conversation_id.contains(':')
        || Uuid::parse_str(conversation_id).is_err()
    {
        return Err(CollabError::InvalidArgument(
            "cursor must be '<positive-sequence>:<target-id>'".into(),
        ));
    }
    Ok(ActivityCursor {
        last_activity_seq,
        conversation_id: conversation_id.to_owned(),
    })
}

fn activity_cursor_for_item(item: &ActivityInboxItem) -> String {
    format!("{}:{}", item.last_activity_seq, item.conversation_id)
}
