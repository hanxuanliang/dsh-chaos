//! Task projection, claim ownership, and lifecycle transitions.

use super::*;
use crate::message::store::MessageStore;

impl CollabCore {
    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        if let Some(target_id) = target_id {
            require_target_access(&connection, target_id, actor_id).await?;
        }
        tasks_for_actor(&connection, actor_id, target_id).await
    }
    /// Convert a committed top-level Message to a Task with a target-local
    /// monotonic task number.
    pub async fn create_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let target_id = MessageStore::new(&transaction)
            .target_of(message_id)
            .await?;
        let route = require_target_access(&transaction, &target_id, actor_id).await?;
        if route.kind == TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "Thread replies cannot become Tasks".into(),
            ));
        }

        if let Some(task) = find_task(&transaction, message_id).await? {
            transaction.commit().await?;
            return Ok(task);
        }

        transaction
            .execute(
                "INSERT INTO target_counters (target_id, next_task_number)
                 VALUES (?1, 1)
                 ON CONFLICT(target_id) DO NOTHING",
                [target_id.as_str()],
            )
            .await?;
        let mut rows = transaction
            .query(
                "SELECT next_task_number FROM target_counters WHERE target_id = ?1",
                [target_id.as_str()],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::Database(
                "target task counter disappeared inside transaction".into(),
            ));
        };
        let number = row.get::<i64>(0)?;
        drop(rows);
        transaction
            .execute(
                "UPDATE target_counters SET next_task_number = ?2 WHERE target_id = ?1",
                (target_id.as_str(), number + 1),
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO tasks
                 (message_id, target_id, number, status, assignee_id, version, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, 'todo', NULL, 1, ?4, ?4)",
                (message_id, target_id.as_str(), number, now),
            )
            .await?;
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "created",
            None,
            Some(TaskStatus::Todo),
            None,
            None,
            1,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskCreated,
            &target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        let anchor_text = MessageStore::new(&transaction)
            .body_text(message_id)
            .await?;
        transaction.commit().await?;

        Ok(Task {
            message_id: message_id.to_owned(),
            target_id,
            number,
            status: TaskStatus::Todo,
            assignee_id: None,
            version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            anchor_text,
        })
    }

    /// Claim an unowned Task. A second actor receives a typed concurrency
    /// conflict; the current assignee may repeat the claim idempotently.
    pub async fn claim_task(&self, message_id: &str, actor_id: &str) -> Result<Task> {
        self.assert_open()?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(&transaction, &current.target_id, actor_id).await?;
        if current.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }
        if let Some(assignee_id) = &current.assignee_id {
            if assignee_id == actor_id {
                transaction.commit().await?;
                return Ok(current);
            }
            return Err(CollabError::TaskAlreadyClaimed {
                message_id: message_id.to_owned(),
                assignee_id: assignee_id.clone(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET assignee_id = ?2, status = 'in_progress', version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND assignee_id IS NULL AND version = ?5",
                (message_id, actor_id, next_version, now, current.version),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task claim compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "claimed",
            Some(current.status),
            Some(TaskStatus::InProgress),
            None,
            Some(actor_id),
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            message_id: message_id.to_owned(),
            target_id: current.target_id,
            number: current.number,
            status: TaskStatus::InProgress,
            assignee_id: Some(actor_id.to_owned()),
            version: next_version,
            created_at_ms: current.created_at_ms,
            updated_at_ms: now,
            anchor_text: current.anchor_text,
        })
    }

    /// Release a Task currently claimed by this actor while preserving its
    /// independent status. The expected version fences stale UI writes.
    pub async fn unclaim_task(
        &self,
        message_id: &str,
        actor_id: &str,
        expected_version: i64,
    ) -> Result<Task> {
        self.assert_open()?;
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(&transaction, &current.target_id, actor_id).await?;
        if current.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: message_id.to_owned(),
                expected: expected_version,
                actual: current.version,
            });
        }
        let Some(assignee_id) = current.assignee_id.as_deref() else {
            transaction.commit().await?;
            return Ok(current);
        };
        if assignee_id != actor_id {
            return Err(CollabError::PermissionDenied {
                actor_id: actor_id.to_owned(),
                action: "unclaim another actor's task in",
                target_id: current.target_id,
            });
        }
        if current.status == TaskStatus::Done {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET assignee_id = NULL, version = ?2, updated_at_ms = ?3
                 WHERE message_id = ?1 AND assignee_id = ?4 AND version = ?5",
                (message_id, next_version, now, actor_id, current.version),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task unclaim compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "unclaimed",
            Some(current.status),
            Some(current.status),
            Some(actor_id),
            None,
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            assignee_id: None,
            version: next_version,
            updated_at_ms: now,
            ..current
        })
    }

    /// Move one Task through the explicit lifecycle with optimistic version
    /// fencing. Assignment remains unchanged.
    pub async fn update_task_status(
        &self,
        message_id: &str,
        actor_id: &str,
        status: TaskStatus,
        expected_version: i64,
    ) -> Result<Task> {
        self.assert_open()?;
        if expected_version < 1 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        require_actor(&transaction, actor_id).await?;
        let current = find_task(&transaction, message_id)
            .await?
            .ok_or_else(|| not_found("task", message_id))?;
        require_target_access(&transaction, &current.target_id, actor_id).await?;
        if current.version != expected_version {
            return Err(CollabError::TaskVersionConflict {
                message_id: message_id.to_owned(),
                expected: expected_version,
                actual: current.version,
            });
        }
        if current
            .assignee_id
            .as_deref()
            .is_some_and(|assignee_id| assignee_id != actor_id)
            && !is_owner(&transaction, &current.target_id, actor_id).await?
        {
            return Err(CollabError::PermissionDenied {
                actor_id: actor_id.to_owned(),
                action: "update another actor's task in",
                target_id: current.target_id,
            });
        }
        if current.status == status {
            transaction.commit().await?;
            return Ok(current);
        }
        if !task_transition_allowed(current.status, status) {
            return Err(CollabError::TaskTransitionDenied {
                message_id: message_id.to_owned(),
                status: current.status.as_str().to_owned(),
            });
        }

        let next_version = current.version + 1;
        let changed = transaction
            .execute(
                "UPDATE tasks
                 SET status = ?2, version = ?3, updated_at_ms = ?4
                 WHERE message_id = ?1 AND version = ?5",
                (
                    message_id,
                    status.as_str(),
                    next_version,
                    now,
                    current.version,
                ),
            )
            .await?;
        if changed != 1 {
            return Err(CollabError::Database(
                "task status compare-and-set did not update one row".into(),
            ));
        }
        insert_task_event(
            &transaction,
            message_id,
            actor_id,
            "status_changed",
            Some(current.status),
            Some(status),
            current.assignee_id.as_deref(),
            current.assignee_id.as_deref(),
            next_version,
            now,
        )
        .await?;
        insert_target_change(
            &transaction,
            ChangeKind::TaskUpdated,
            &current.target_id,
            message_id,
            &[],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(Task {
            status,
            version: next_version,
            updated_at_ms: now,
            ..current
        })
    }
}

pub(crate) async fn find_task(connection: &Connection, message_id: &str) -> Result<Option<Task>> {
    let mut rows = connection
        .query(
            "SELECT task.message_id, task.target_id, task.number, task.status, task.assignee_id,
                    task.version, task.created_at_ms, task.updated_at_ms, message.body_json
             FROM tasks task
             LEFT JOIN messages message ON message.id = task.message_id
             WHERE task.message_id = ?1",
            [message_id],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| task_from_row(&row))
        .transpose()
}

pub(crate) fn task_from_row(row: &Row) -> Result<Task> {
    let message_id = row.get::<String>(0)?;
    let status_text = row.get::<String>(3)?;
    let status = TaskStatus::parse(&status_text).ok_or_else(|| {
        CollabError::Database(format!(
            "task '{message_id}' has invalid status '{status_text}'"
        ))
    })?;
    let anchor_text = row
        .get::<Option<String>>(8)?
        .map(|body_json| {
            serde_json::from_str::<StoredTextBody>(&body_json)
                .map(|body| body.text)
                .map_err(|error| {
                    CollabError::Database(format!(
                        "task '{message_id}' anchor has invalid body: {error}"
                    ))
                })
        })
        .transpose()?;
    Ok(Task {
        message_id,
        target_id: row.get(1)?,
        number: row.get(2)?,
        status,
        assignee_id: row.get(4)?,
        version: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        anchor_text,
    })
}

pub(crate) async fn tasks_for_actor(
    connection: &Connection,
    actor_id: &str,
    target_id: Option<&str>,
) -> Result<Vec<Task>> {
    let statement = if target_id.is_some() {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms,
                message.body_json
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
         LEFT JOIN messages message ON message.id = task.message_id
         WHERE task.target_id = ?2 AND target.archived_at_ms IS NULL
         ORDER BY task.number"
    } else {
        "SELECT task.message_id, task.target_id, task.number, task.status,
                task.assignee_id, task.version, task.created_at_ms, task.updated_at_ms,
                message.body_json
         FROM tasks task
         JOIN targets target ON target.id = task.target_id
         JOIN memberships membership
           ON membership.target_id = CASE
             WHEN target.kind = 'thread' THEN target.parent_target_id
             ELSE target.id
           END
          AND membership.actor_id = ?1
          AND membership.left_at_ms IS NULL
         LEFT JOIN messages message ON message.id = task.message_id
         WHERE target.archived_at_ms IS NULL
         ORDER BY task.target_id, task.number"
    };
    let mut rows = if let Some(target_id) = target_id {
        connection.query(statement, (actor_id, target_id)).await?
    } else {
        connection.query(statement, [actor_id]).await?
    };
    let mut tasks = Vec::new();
    while let Some(row) = rows.next().await? {
        tasks.push(task_from_row(&row)?);
    }
    Ok(tasks)
}

pub(crate) fn task_transition_allowed(from: TaskStatus, to: TaskStatus) -> bool {
    matches!(
        (from, to),
        (TaskStatus::Todo, TaskStatus::InProgress)
            | (
                TaskStatus::InProgress,
                TaskStatus::Todo | TaskStatus::InReview
            )
            | (
                TaskStatus::InReview,
                TaskStatus::InProgress | TaskStatus::Done
            )
            | (TaskStatus::Done, TaskStatus::InProgress)
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_task_event(
    connection: &Connection,
    message_id: &str,
    actor_id: &str,
    event_type: &str,
    from_status: Option<TaskStatus>,
    to_status: Option<TaskStatus>,
    from_assignee_id: Option<&str>,
    to_assignee_id: Option<&str>,
    task_version: i64,
    created_at_ms: i64,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO task_events
             (message_id, actor_id, event_type, from_status, to_status,
              from_assignee_id, to_assignee_id, task_version, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                message_id,
                actor_id,
                event_type,
                from_status.map(TaskStatus::as_str),
                to_status.map(TaskStatus::as_str),
                from_assignee_id,
                to_assignee_id,
                task_version,
                created_at_ms,
            ),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn task_reads_carry_authoritative_anchor_text() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let anchor = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "anchor-1".into(),
                text: "anchor body survives paging".into(),
            })
            .await?
            .message;
        let created = core.create_task(&anchor.id, &user.id).await?;
        assert_eq!(
            created.anchor_text.as_deref(),
            Some("anchor body survives paging")
        );

        // Push the anchor far outside any recent-message page; the Task read
        // still resolves the true anchor body from the store.
        for index in 0..120 {
            core.send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: format!("filler-{index}"),
                text: format!("filler {index}"),
            })
            .await?;
        }
        let tasks = core.list_tasks(&alpha.id, Some(&channel.id)).await?;
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].anchor_text.as_deref(),
            Some("anchor body survives paging")
        );

        // Mutations keep the anchor attached.
        let claimed = core.claim_task(&anchor.id, &alpha.id).await?;
        assert_eq!(
            claimed.anchor_text.as_deref(),
            Some("anchor body survives paging")
        );
        Ok(())
    }

    #[tokio::test]
    async fn task_lifecycle_uses_version_fencing_and_emits_changes() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let before = core.snapshot(&user.id).await?.cursor;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "task-lifecycle-message".into(),
                text: "finish the lifecycle".into(),
            })
            .await?;
        let created = core.create_task(&sent.message.id, &user.id).await?;
        let claimed = core.claim_task(&sent.message.id, &alpha.id).await?;
        assert_eq!(claimed.version, created.version + 1);
        assert!(matches!(
            core.update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::InReview,
                claimed.version,
            )
            .await,
            Err(CollabError::PermissionDenied { .. })
        ));

        let owner_review = core
            .update_task_status(
                &sent.message.id,
                &user.id,
                TaskStatus::InReview,
                claimed.version,
            )
            .await?;
        let reopened = core
            .update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::InProgress,
                owner_review.version,
            )
            .await?;
        let review = core
            .update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::InReview,
                reopened.version,
            )
            .await?;
        assert!(matches!(
            core.update_task_status(
                &sent.message.id,
                &alpha.id,
                TaskStatus::Done,
                claimed.version,
            )
            .await,
            Err(CollabError::TaskVersionConflict { .. })
        ));
        let unclaimed = core
            .unclaim_task(&sent.message.id, &alpha.id, review.version)
            .await?;
        assert_eq!(unclaimed.status, TaskStatus::InReview);
        assert!(unclaimed.assignee_id.is_none());

        let beta_claim = core.claim_task(&sent.message.id, &beta.id).await?;
        let beta_review = core
            .update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::InReview,
                beta_claim.version,
            )
            .await?;
        let done = core
            .update_task_status(
                &sent.message.id,
                &beta.id,
                TaskStatus::Done,
                beta_review.version,
            )
            .await?;
        assert!(matches!(
            core.unclaim_task(&sent.message.id, &beta.id, done.version)
                .await,
            Err(CollabError::TaskTransitionDenied { .. })
        ));
        assert_eq!(
            core.list_tasks(&user.id, Some(&channel.id)).await?,
            vec![done]
        );
        let changes = core.list_changes(&user.id, before, 50).await?;
        assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == ChangeKind::TaskCreated)
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == ChangeKind::TaskUpdated)
                .count(),
            8
        );
        Ok(())
    }

    #[tokio::test]
    async fn only_one_concurrent_task_claim_wins() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id,
                author_id: user.id.clone(),
                client_request_id: "task-message".into(),
                text: "implement it".into(),
            })
            .await?;
        core.create_task(&sent.message.id, &user.id).await?;

        let alpha_claim = core.claim_task(&sent.message.id, &alpha.id);
        let beta_claim = core.claim_task(&sent.message.id, &beta.id);
        let (alpha_result, beta_result) = tokio::join!(alpha_claim, beta_claim);
        let successes = usize::from(alpha_result.is_ok()) + usize::from(beta_result.is_ok());
        let conflicts = usize::from(matches!(
            alpha_result,
            Err(CollabError::TaskAlreadyClaimed { .. })
        )) + usize::from(matches!(
            beta_result,
            Err(CollabError::TaskAlreadyClaimed { .. })
        ));
        assert_eq!(successes, 1);
        assert_eq!(conflicts, 1);
        Ok(())
    }
}
