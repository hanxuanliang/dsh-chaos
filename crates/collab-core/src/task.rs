//! Task projection, claim ownership, and lifecycle transitions.

use super::*;

impl CollabCore {
    /// List Task metadata visible to one actor, optionally narrowed to an
    /// exact target.
    pub async fn list_tasks(&self, actor_id: &str, target_id: Option<&str>) -> Result<Vec<Task>> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_actor(&connection, actor_id).await?;
        if let Some(target_id) = target_id {
            require_target_access(&connection, target_id, actor_id, "list tasks in").await?;
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
        let target_id = message_target(&transaction, message_id).await?;
        let route =
            require_target_access(&transaction, &target_id, actor_id, "create task").await?;
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
        let anchor_text = message_body_text(&transaction, message_id).await?;
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
        require_target_access(&transaction, &current.target_id, actor_id, "claim task").await?;
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
        require_target_access(
            &transaction,
            &current.target_id,
            actor_id,
            "unclaim task in",
        )
        .await?;
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
        require_target_access(&transaction, &current.target_id, actor_id, "update task in").await?;
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
