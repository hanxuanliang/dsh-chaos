//! Channel, Direct, and Thread target operations and authorization routes.

use super::*;

impl CollabCore {
    /// Create a Channel and make its creator the owner/member.
    pub async fn create_channel(&self, name: &str, creator_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("channel name", name)?;
        require_non_empty("creator_id", creator_id)?;

        let now = now_ms()?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Channel,
            name: name.to_owned(),
            parent_target_id: None,
            root_message_id: None,
            created_by: creator_id.to_owned(),
            created_at_ms: now,
        };

        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, creator_id).await?;
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL)",
                (
                    target.id.as_str(),
                    target.kind.as_str(),
                    target.name.as_str(),
                    target.created_by.as_str(),
                    target.created_at_ms,
                ),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO memberships
                 (target_id, actor_id, role, joined_at_ms, left_at_ms)
                 VALUES (?1, ?2, 'owner', ?3, NULL)",
                (target.id.as_str(), target.created_by.as_str(), now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Return the one stable Direct target for an unordered pair of actors,
    /// creating it and its two memberships when absent.
    pub async fn create_direct(&self, actor_id: &str, peer_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        require_non_empty("peer_id", peer_id)?;
        if actor_id == peer_id {
            return Err(CollabError::InvalidArgument(
                "a Direct target requires two distinct actors".into(),
            ));
        }
        let (actor_low_id, actor_high_id) = if actor_id < peer_id {
            (actor_id, peer_id)
        } else {
            (peer_id, actor_id)
        };
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        require_actor(&transaction, peer_id).await?;

        let mut rows = transaction
            .query(
                "SELECT target_id FROM direct_pairs
                 WHERE actor_low_id = ?1 AND actor_high_id = ?2",
                (actor_low_id, actor_high_id),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let target_id = row.get::<String>(0)?;
            drop(rows);
            let target = find_target(&transaction, &target_id).await?;
            transaction.commit().await?;
            return Ok(target);
        }
        drop(rows);

        let low_handle = actor_handle(&transaction, actor_low_id).await?;
        let high_handle = actor_handle(&transaction, actor_high_id).await?;
        let target = Target {
            id: new_id(),
            kind: TargetKind::Direct,
            name: format!("@{low_handle} ↔ @{high_handle}"),
            parent_target_id: None,
            root_message_id: None,
            created_by: actor_id.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, 'direct', ?2, NULL, NULL, ?3, ?4, NULL)",
                (
                    target.id.as_str(),
                    target.name.as_str(),
                    target.created_by.as_str(),
                    now,
                ),
            )
            .await?;
        for (member_id, role) in [(actor_id, "owner"), (peer_id, "member")] {
            transaction
                .execute(
                    "INSERT INTO memberships
                     (target_id, actor_id, role, joined_at_ms, left_at_ms)
                     VALUES (?1, ?2, ?3, ?4, NULL)",
                    (target.id.as_str(), member_id, role, now),
                )
                .await?;
        }
        transaction
            .execute(
                "INSERT INTO direct_pairs
                 (target_id, actor_low_id, actor_high_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                (target.id.as_str(), actor_low_id, actor_high_id, now),
            )
            .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TargetCreated,
            &target.id,
            &target.id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Return the one Thread target rooted at a top-level Message. The creator
    /// and root author follow it immediately when they retain parent access.
    pub async fn create_thread(&self, root_message_id: &str, actor_id: &str) -> Result<Target> {
        self.assert_open()?;
        require_non_empty("root_message_id", root_message_id)?;
        require_non_empty("actor_id", actor_id)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let (parent_target_id, root_author_id) =
            message_target_author(&transaction, root_message_id).await?;
        if require_target(&transaction, &parent_target_id).await? == TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "Threads cannot be nested under Thread messages".into(),
            ));
        }
        require_active_member(
            &transaction,
            &parent_target_id,
            actor_id,
            "create Thread in",
        )
        .await?;

        let mut rows = transaction
            .query(
                "SELECT id FROM targets WHERE root_message_id = ?1",
                [root_message_id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let target_id = row.get::<String>(0)?;
            drop(rows);
            let target = find_target(&transaction, &target_id).await?;
            transaction.commit().await?;
            return Ok(target);
        }
        drop(rows);

        let target = Target {
            id: new_id(),
            kind: TargetKind::Thread,
            name: format!("thread:{root_message_id}"),
            parent_target_id: Some(parent_target_id.clone()),
            root_message_id: Some(root_message_id.to_owned()),
            created_by: actor_id.to_owned(),
            created_at_ms: now,
        };
        transaction
            .execute(
                "INSERT INTO targets
                 (id, kind, name, parent_target_id, root_message_id, created_by, created_at_ms, archived_at_ms)
                 VALUES (?1, 'thread', ?2, ?3, ?4, ?5, ?6, NULL)",
                (
                    target.id.as_str(),
                    target.name.as_str(),
                    parent_target_id.as_str(),
                    root_message_id,
                    actor_id,
                    now,
                ),
            )
            .await?;
        follow_thread_in_transaction(&transaction, &target.id, actor_id, now).await?;
        if root_author_id != actor_id
            && is_active_member(&transaction, &parent_target_id, &root_author_id).await?
        {
            follow_thread_in_transaction(&transaction, &target.id, &root_author_id, now).await?;
        }
        let parent_actor_ids = active_member_ids(&transaction, &parent_target_id).await?;
        insert_change(
            &transaction,
            ChangeKind::TargetCreated,
            Some(&target.id),
            &target.id,
            &parent_actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Batch summaries for Threads rooted at the given top-level Messages
    /// (tae GET thread-summaries 等价物): one call returns the reply count
    /// plus up to 3 most-recent distinct repliers for up to 100 roots.
    /// Roots without a Thread, zero-reply Threads, and Threads whose parent
    /// the actor cannot read are omitted — previews never fail loudly on
    /// partial data.
    pub async fn thread_summaries(
        &self,
        actor_id: &str,
        root_message_ids: &[String],
    ) -> Result<Vec<ThreadSummary>> {
        self.assert_open()?;
        require_non_empty("actor_id", actor_id)?;
        if root_message_ids.len() > 100 {
            return Err(CollabError::InvalidArgument(
                "root_message_ids must contain at most 100 ids".into(),
            ));
        }
        if root_message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut unique: Vec<String> = Vec::new();
        for id in root_message_ids {
            // 后端 uuid 形态约束：SQL IN 字面量从受控字符集构造。
            if id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(CollabError::InvalidArgument(
                    "root_message_id must be a backend-assigned id".into(),
                ));
            }
            if !unique.contains(id) {
                unique.push(id.clone());
            }
        }
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;

        let quoted = unique
            .iter()
            .map(|id| format!("'{}'", id))
            .collect::<Vec<String>>()
            .join(", ");
        let counts_sql = format!(
            "SELECT t.id, t.root_message_id, t.parent_target_id, COUNT(m.id), MAX(m.created_at_ms)              FROM targets t LEFT JOIN messages m ON m.target_id = t.id              WHERE t.root_message_id IN ({}) GROUP BY t.id",
            quoted,
        );
        let mut rows = connection.query(&counts_sql, ()).await?;
        let mut summaries: Vec<ThreadSummary> = Vec::new();
        let mut visible_thread_ids: Vec<String> = Vec::new();
        while let Some(row) = rows.next().await? {
            let thread_id: String = row.get(0)?;
            let root_message_id: String = row.get(1)?;
            let parent_target_id: Option<String> = row.get(2)?;
            let reply_count: i64 = row.get(3)?;
            let last_reply_at_ms: Option<i64> = row.get(4)?;
            let visible = match parent_target_id.as_deref() {
                Some(parent) => {
                    is_active_member(&connection, parent, actor_id).await?
                        || is_active_member(&connection, &thread_id, actor_id).await?
                }
                None => is_active_member(&connection, &thread_id, actor_id).await?,
            };
            if !visible || reply_count == 0 {
                continue;
            }
            visible_thread_ids.push(thread_id.clone());
            summaries.push(ThreadSummary {
                root_message_id,
                thread_id,
                reply_count,
                last_reply_at_ms,
                recent_replier_ids: Vec::new(),
            });
        }

        // 最近 3 个不同回复者（按各自最近一次出现的 seq 排序）。
        if !visible_thread_ids.is_empty() {
            let quoted_threads = visible_thread_ids
                .iter()
                .map(|id| format!("'{}'", id))
                .collect::<Vec<String>>()
                .join(", ");
            let repliers_sql = format!(
                "SELECT target_id, author_id FROM (                    SELECT m.target_id, m.author_id,                           ROW_NUMBER() OVER (PARTITION BY m.target_id ORDER BY m.seq DESC) AS rn                    FROM messages m WHERE m.target_id IN ({})                  ) GROUP BY target_id, author_id ORDER BY target_id, rn",
                quoted_threads,
            );
            let mut rows = connection.query(&repliers_sql, ()).await?;
            while let Some(row) = rows.next().await? {
                let target_id: String = row.get(0)?;
                let author_id: String = row.get(1)?;
                if let Some(summary) = summaries
                    .iter_mut()
                    .find(|summary| summary.thread_id == target_id)
                    .filter(|summary| summary.recent_replier_ids.len() < 3)
                {
                    summary.recent_replier_ids.push(author_id);
                }
            }
        }
        Ok(summaries)
    }

    /// Follow one Thread after rechecking access to its parent target.
    pub async fn follow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route = require_target_route(&transaction, thread_target_id).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "follow_thread requires a Thread target".into(),
            ));
        }
        require_active_member(
            &transaction,
            route.permission_target_id(thread_target_id),
            actor_id,
            "follow",
        )
        .await?;
        if follow_thread_in_transaction(&transaction, thread_target_id, actor_id, now).await? {
            insert_target_change(
                &transaction,
                ChangeKind::ThreadFollowChanged,
                thread_target_id,
                actor_id,
                &[actor_id],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Stop future ordinary Thread delivery for one current parent member.
    pub async fn unfollow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let route = require_target_route(&transaction, thread_target_id).await?;
        if route.kind != TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "unfollow_thread requires a Thread target".into(),
            ));
        }
        require_active_member(
            &transaction,
            route.permission_target_id(thread_target_id),
            actor_id,
            "unfollow",
        )
        .await?;
        let changed = transaction
            .execute(
                "UPDATE thread_follows
                 SET unfollowed_at_ms = ?3
                 WHERE thread_target_id = ?1 AND actor_id = ?2
                   AND unfollowed_at_ms IS NULL",
                (thread_target_id, actor_id, now),
            )
            .await?;
        if changed == 1 {
            insert_target_change(
                &transaction,
                ChangeKind::ThreadFollowChanged,
                thread_target_id,
                actor_id,
                &[actor_id],
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TargetRoute {
    pub(crate) kind: TargetKind,
    pub(crate) parent_target_id: Option<String>,
}

impl TargetRoute {
    pub(crate) fn permission_target_id<'a>(&'a self, exact_target_id: &'a str) -> &'a str {
        self.parent_target_id.as_deref().unwrap_or(exact_target_id)
    }
}

pub(crate) fn parse_target_kind(target_id: &str, value: &str) -> Result<TargetKind> {
    match value {
        "channel" => Ok(TargetKind::Channel),
        "direct" => Ok(TargetKind::Direct),
        "thread" => Ok(TargetKind::Thread),
        other => Err(CollabError::Database(format!(
            "target '{target_id}' has unknown kind '{other}'"
        ))),
    }
}

pub(crate) async fn require_target_route(
    connection: &Connection,
    target_id: &str,
) -> Result<TargetRoute> {
    let mut rows = connection
        .query(
            "SELECT kind, parent_target_id
             FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
            [target_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("active target", target_id));
    };
    let kind_text = row.get::<String>(0)?;
    let parent_target_id = row.get::<Option<String>>(1)?;
    drop(rows);
    let kind = parse_target_kind(target_id, &kind_text)?;
    if kind == TargetKind::Thread && parent_target_id.is_none() {
        return Err(CollabError::Database(format!(
            "Thread target '{target_id}' has no parent target"
        )));
    }
    if kind != TargetKind::Thread && parent_target_id.is_some() {
        return Err(CollabError::Database(format!(
            "non-Thread target '{target_id}' unexpectedly has a parent"
        )));
    }
    if let Some(parent_target_id) = parent_target_id.as_deref() {
        let mut parent_rows = connection
            .query(
                "SELECT kind FROM targets
                 WHERE id = ?1 AND archived_at_ms IS NULL",
                [parent_target_id],
            )
            .await?;
        let Some(parent_row) = parent_rows.next().await? else {
            return Err(not_found("active Thread parent target", parent_target_id));
        };
        let parent_kind_text = parent_row.get::<String>(0)?;
        let parent_kind = parse_target_kind(parent_target_id, &parent_kind_text)?;
        if parent_kind == TargetKind::Thread {
            return Err(CollabError::Database(format!(
                "Thread target '{target_id}' has a Thread parent"
            )));
        }
    }
    Ok(TargetRoute {
        kind,
        parent_target_id,
    })
}

pub(crate) async fn require_target(connection: &Connection, target_id: &str) -> Result<TargetKind> {
    Ok(require_target_route(connection, target_id).await?.kind)
}

pub(crate) async fn require_target_access(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
    action: &'static str,
) -> Result<TargetRoute> {
    let route = require_target_route(connection, target_id).await?;
    require_active_member(
        connection,
        route.permission_target_id(target_id),
        actor_id,
        action,
    )
    .await?;
    Ok(route)
}

pub(crate) async fn require_owner(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<()> {
    if is_owner(connection, target_id, actor_id).await? {
        return Ok(());
    }
    Err(CollabError::PermissionDenied {
        actor_id: actor_id.to_owned(),
        action: "manage",
        target_id: target_id.to_owned(),
    })
}

pub(crate) async fn is_owner(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2
               AND role = 'owner' AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

pub(crate) async fn require_active_member(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
    action: &'static str,
) -> Result<()> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    if rows.next().await?.is_none() {
        return Err(CollabError::PermissionDenied {
            actor_id: actor_id.to_owned(),
            action,
            target_id: target_id.to_owned(),
        });
    }
    Ok(())
}

pub(crate) async fn is_active_member(
    connection: &Connection,
    target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM memberships
             WHERE target_id = ?1 AND actor_id = ?2 AND left_at_ms IS NULL",
            (target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

pub(crate) async fn actor_handle(connection: &Connection, actor_id: &str) -> Result<String> {
    let mut rows = connection
        .query("SELECT handle FROM actors WHERE id = ?1", [actor_id])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Err(not_found("actor", actor_id)),
    }
}

pub(crate) async fn find_target(connection: &Connection, target_id: &str) -> Result<Target> {
    let mut rows = connection
        .query(
            "SELECT id, kind, name, parent_target_id, root_message_id,
                    created_by, created_at_ms
             FROM targets WHERE id = ?1 AND archived_at_ms IS NULL",
            [target_id],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(not_found("active target", target_id));
    };
    let kind_text = row.get::<String>(1)?;
    Ok(Target {
        id: row.get(0)?,
        kind: parse_target_kind(target_id, &kind_text)?,
        name: row.get(2)?,
        parent_target_id: row.get(3)?,
        root_message_id: row.get(4)?,
        created_by: row.get(5)?,
        created_at_ms: row.get(6)?,
    })
}

pub(crate) async fn targets_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<Target>> {
    let mut rows = connection
        .query(
            "SELECT DISTINCT target.id, target.kind, target.name,
                    target.parent_target_id, target.root_message_id,
                    target.created_by, target.created_at_ms
             FROM targets target
             JOIN memberships membership
               ON membership.target_id = CASE
                 WHEN target.kind = 'thread' THEN target.parent_target_id
                 ELSE target.id
               END
              AND membership.actor_id = ?1
              AND membership.left_at_ms IS NULL
             WHERE target.archived_at_ms IS NULL
               AND (
                 target.kind <> 'thread'
                 OR EXISTS (
                   SELECT 1 FROM targets parent
                   WHERE parent.id = target.parent_target_id
                     AND parent.kind IN ('channel', 'direct')
                     AND parent.archived_at_ms IS NULL
                 )
               )
             ORDER BY target.created_at_ms, target.id",
            [actor_id],
        )
        .await?;
    let mut targets = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = row.get::<String>(0)?;
        let kind_text = row.get::<String>(1)?;
        targets.push(Target {
            kind: parse_target_kind(&id, &kind_text)?,
            id,
            name: row.get(2)?,
            parent_target_id: row.get(3)?,
            root_message_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at_ms: row.get(6)?,
        });
    }
    Ok(targets)
}

pub(crate) async fn follow_thread_in_transaction(
    connection: &Connection,
    thread_target_id: &str,
    actor_id: &str,
    now: i64,
) -> Result<bool> {
    let changed = connection
        .execute(
            "INSERT INTO thread_follows
             (thread_target_id, actor_id, followed_at_ms, unfollowed_at_ms)
             VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(thread_target_id, actor_id) DO UPDATE SET
               followed_at_ms = excluded.followed_at_ms,
               unfollowed_at_ms = NULL
             WHERE thread_follows.unfollowed_at_ms IS NOT NULL",
            (thread_target_id, actor_id, now),
        )
        .await?;
    Ok(changed == 1)
}

pub(crate) async fn is_following_thread(
    connection: &Connection,
    thread_target_id: &str,
    actor_id: &str,
) -> Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM thread_follows
             WHERE thread_target_id = ?1
               AND actor_id = ?2
               AND unfollowed_at_ms IS NULL",
            (thread_target_id, actor_id),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

pub(crate) async fn followed_thread_ids_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT f.thread_target_id
             FROM thread_follows f
             JOIN targets t
               ON t.id = f.thread_target_id AND t.kind = 'thread'
             JOIN targets parent
               ON parent.id = t.parent_target_id
              AND parent.kind IN ('channel', 'direct')
             JOIN memberships m
               ON m.target_id = t.parent_target_id AND m.actor_id = f.actor_id
             WHERE f.actor_id = ?1
               AND f.unfollowed_at_ms IS NULL
               AND t.archived_at_ms IS NULL
               AND parent.archived_at_ms IS NULL
               AND m.left_at_ms IS NULL
             ORDER BY t.created_at_ms DESC, f.thread_target_id",
            [actor_id],
        )
        .await?;
    let mut target_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        target_ids.push(row.get(0)?);
    }
    Ok(target_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn direct_target_is_unique_for_an_unordered_actor_pair() -> Result<()> {
        let (core, user, alpha, beta, _channel) = fixture().await?;
        let direct = core.create_direct(&alpha.id, &beta.id).await?;
        let reversed = core.create_direct(&beta.id, &alpha.id).await?;
        assert_eq!(direct, reversed);
        assert_eq!(direct.kind, TargetKind::Direct);
        assert!(direct.parent_target_id.is_none());
        assert!(direct.root_message_id.is_none());

        let sent = core
            .send_message(SendMessageRequest {
                target_id: direct.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "direct-message".into(),
                text: "only beta receives this".into(),
            })
            .await?;
        assert_eq!(sent.recipient_ids, vec![beta.id.clone()]);
        assert_eq!(
            core.read_messages(&beta.id, &direct.id, 0, 10).await?,
            vec![sent.message]
        );
        assert!(matches!(
            core.read_messages(&user.id, &direct.id, 0, 10).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.add_member(&direct.id, &user.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn thread_inherits_parent_access_and_delivers_only_to_followers() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let outsider = core.create_user("thread-outsider", "Outsider").await?;
        let root = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "thread-root".into(),
                text: "review in a thread".into(),
            })
            .await?;
        let beta_binding = core
            .bind_runtime(
                &beta.id,
                "thread-beta-session",
                "openai",
                "codex",
                "default",
            )
            .await?;
        let root_batch = core
            .check_inbox(
                &beta.id,
                beta_binding.generation,
                &beta_binding.session_id,
                10,
            )
            .await?;
        core.mark_model_seen(
            root_batch.id.as_deref().expect("root delivery batch"),
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
        )
        .await?;
        let beta_change_cursor = core.snapshot(&beta.id).await?.cursor;
        let thread = core.create_thread(&root.message.id, &alpha.id).await?;
        assert_eq!(thread.kind, TargetKind::Thread);
        assert_eq!(
            thread.parent_target_id.as_deref(),
            Some(channel.id.as_str())
        );
        assert_eq!(
            thread.root_message_id.as_deref(),
            Some(root.message.id.as_str())
        );
        assert_eq!(
            core.create_thread(&root.message.id, &beta.id).await?,
            thread
        );
        let beta_thread_changes = core.list_changes(&beta.id, beta_change_cursor, 50).await?;
        assert!(beta_thread_changes.iter().any(|change| {
            change.kind == ChangeKind::TargetCreated && change.entity_id == thread.id
        }));

        let first_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-1".into(),
                text: "alpha reply".into(),
            })
            .await?;
        assert_eq!(first_reply.recipient_ids, vec![user.id.clone()]);
        assert_eq!(
            core.snapshot(&alpha.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        assert!(
            core.snapshot(&beta.id)
                .await?
                .followed_thread_ids
                .is_empty()
        );

        core.follow_thread(&thread.id, &beta.id).await?;
        assert_eq!(
            core.snapshot(&beta.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        let second_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "thread-reply-2".into(),
                text: "owner reply".into(),
            })
            .await?;
        assert_eq!(
            second_reply.recipient_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert!(
            core.list_pending_wakes(10)
                .await?
                .iter()
                .any(|wake| wake.binding.agent_id == beta.id)
        );
        let thread_batch = core
            .check_inbox(
                &beta.id,
                beta_binding.generation,
                &beta_binding.session_id,
                10,
            )
            .await?;
        assert_eq!(thread_batch.messages.len(), 1);
        assert_eq!(thread_batch.messages[0].message.id, second_reply.message.id);
        core.mark_model_seen(
            thread_batch.id.as_deref().expect("Thread delivery batch"),
            &beta.id,
            beta_binding.generation,
            &beta_binding.session_id,
        )
        .await?;

        core.unfollow_thread(&thread.id, &beta.id).await?;
        assert!(
            core.snapshot(&beta.id)
                .await?
                .followed_thread_ids
                .is_empty()
        );
        let beta_unfollowed_cursor = core.snapshot(&beta.id).await?.cursor;
        assert_eq!(
            core.read_message(&beta.id, &thread.id, &second_reply.message.id)
                .await?,
            second_reply.message
        );
        let after_unfollow = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-3".into(),
                text: "beta should not receive this".into(),
            })
            .await?;
        assert_eq!(after_unfollow.recipient_ids, vec![user.id.clone()]);
        let beta_realtime_changes = core
            .list_changes(&beta.id, beta_unfollowed_cursor, 50)
            .await?;
        assert!(beta_realtime_changes.iter().any(|change| {
            change.kind == ChangeKind::MessageCreated
                && change.entity_id == after_unfollow.message.id
        }));

        let beta_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: beta.id.clone(),
                client_request_id: "thread-reply-4".into(),
                text: "participating follows again".into(),
            })
            .await?;
        let mut expected_beta_reply_recipients = vec![user.id.clone(), alpha.id.clone()];
        expected_beta_reply_recipients.sort();
        assert_eq!(beta_reply.recipient_ids, expected_beta_reply_recipients);
        let final_reply = core
            .send_message(SendMessageRequest {
                target_id: thread.id.clone(),
                author_id: alpha.id.clone(),
                client_request_id: "thread-reply-5".into(),
                text: "beta follows again".into(),
            })
            .await?;
        assert!(final_reply.recipient_ids.contains(&beta.id));
        assert_eq!(
            core.snapshot(&beta.id).await?.followed_thread_ids,
            vec![thread.id.clone()]
        );
        assert!(matches!(
            core.read_messages(&outsider.id, &thread.id, 0, 10).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.create_task(&first_reply.message.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.create_thread(&first_reply.message.id, &alpha.id).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }
}
