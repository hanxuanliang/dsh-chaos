use turso::Connection;
use turso::params_from_iter;
use turso::transaction::TransactionBehavior;

use crate::actor::require_actor;
use crate::{CollabCore, CollabError, Result};

use super::model::ThreadSummary;

impl CollabCore {
    /// Return reply counts and the three most recent distinct repliers for up
    /// to 100 root Messages. Inaccessible and empty Threads are omitted.
    pub async fn thread_summaries(
        &self,
        actor_id: &str,
        root_message_ids: &[String],
    ) -> Result<Vec<ThreadSummary>> {
        self.assert_open()?;
        crate::require_non_empty("actor_id", actor_id)?;
        let root_message_ids = normalized_backend_ids("root_message_id", root_message_ids, 100)?;
        if root_message_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let root_placeholders = placeholders(root_message_ids.len());
        let actor_parameter = root_message_ids.len() + 1;
        let counts_sql = format!(
            "SELECT thread.id, thread.root_message_id, COUNT(message.id),
                    MAX(message.created_at_ms)
             FROM targets thread
             JOIN targets parent
               ON parent.id = thread.parent_target_id
              AND parent.kind IN ('channel', 'direct')
              AND parent.archived_at_ms IS NULL
             JOIN memberships membership
               ON membership.target_id = parent.id
              AND membership.actor_id = ?{actor_parameter}
              AND membership.left_at_ms IS NULL
             LEFT JOIN messages message ON message.target_id = thread.id
             WHERE thread.kind = 'thread'
               AND thread.archived_at_ms IS NULL
               AND thread.root_message_id IN ({root_placeholders})
             GROUP BY thread.id, thread.root_message_id
             ORDER BY thread.root_message_id, thread.id"
        );
        let mut count_parameters = root_message_ids.clone();
        count_parameters.push(actor_id.to_owned());
        let mut rows = transaction
            .query(&counts_sql, params_from_iter(count_parameters))
            .await?;
        let mut summaries = Vec::new();
        let mut visible_thread_ids = Vec::new();
        while let Some(row) = rows.next().await? {
            let reply_count = row.get::<i64>(2)?;
            if reply_count == 0 {
                continue;
            }
            let thread_id = row.get::<String>(0)?;
            visible_thread_ids.push(thread_id.clone());
            summaries.push(ThreadSummary {
                root_message_id: row.get(1)?,
                thread_id,
                reply_count,
                last_reply_at_ms: row.get(3)?,
                recent_replier_ids: Vec::new(),
            });
        }
        drop(rows);

        if !visible_thread_ids.is_empty() {
            let replier_sql = format!(
                "SELECT target_id, author_id, MAX(seq) AS latest_seq
                 FROM messages
                 WHERE target_id IN ({})
                 GROUP BY target_id, author_id
                 ORDER BY target_id, latest_seq DESC, author_id",
                placeholders(visible_thread_ids.len()),
            );
            let mut rows = transaction
                .query(&replier_sql, params_from_iter(visible_thread_ids))
                .await?;
            while let Some(row) = rows.next().await? {
                let target_id = row.get::<String>(0)?;
                let author_id = row.get::<String>(1)?;
                if let Some(summary) = summaries
                    .iter_mut()
                    .find(|summary| summary.thread_id == target_id)
                    .filter(|summary| summary.recent_replier_ids.len() < 3)
                {
                    summary.recent_replier_ids.push(author_id);
                }
            }
        }
        transaction.commit().await?;
        Ok(summaries)
    }
}

pub(crate) async fn followed_thread_ids_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT follow.thread_target_id
             FROM thread_follows follow
             JOIN targets thread
               ON thread.id = follow.thread_target_id AND thread.kind = 'thread'
             JOIN targets parent
               ON parent.id = thread.parent_target_id
              AND parent.kind IN ('channel', 'direct')
             JOIN memberships membership
               ON membership.target_id = thread.parent_target_id
              AND membership.actor_id = follow.actor_id
             WHERE follow.actor_id = ?1
               AND follow.unfollowed_at_ms IS NULL
               AND thread.archived_at_ms IS NULL
               AND parent.archived_at_ms IS NULL
               AND membership.left_at_ms IS NULL
             ORDER BY thread.created_at_ms DESC, follow.thread_target_id",
            [actor_id],
        )
        .await?;
    let mut target_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        target_ids.push(row.get(0)?);
    }
    Ok(target_ids)
}

fn normalized_backend_ids(name: &str, values: &[String], maximum: usize) -> Result<Vec<String>> {
    if values.len() > maximum {
        return Err(CollabError::InvalidArgument(format!(
            "{name}s must contain at most {maximum} ids"
        )));
    }
    let mut unique = Vec::new();
    for value in values {
        if value.len() > 64
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(CollabError::InvalidArgument(format!(
                "{name} must be a backend-assigned id"
            )));
        }
        if !unique.contains(value) {
            unique.push(value.clone());
        }
    }
    Ok(unique)
}

fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}
