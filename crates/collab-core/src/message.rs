//! Immutable Message writes, recipient snapshots, and authorized history reads.

use super::*;
use crate::thread::store::ThreadStore;

impl CollabCore {
    /// Atomically commit one immutable Message, its recipient snapshot, and
    /// every recipient Agent's level-triggered wake watermark.
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<SendMessageResult> {
        self.send_message_inner(request, SendFailpoint::None).await
    }

    /// Read one exact Message while the actor retains access to the exact
    /// target, inherited from the parent for a Thread.
    pub async fn read_message(
        &self,
        actor_id: &str,
        target_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        self.assert_open()?;
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id).await?;
        let mut rows = connection
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM messages WHERE id = ?1 AND target_id = ?2",
                (message_id, target_id),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(not_found("message in exact target", message_id));
        };
        message_from_row(&row)
    }

    /// Read an ascending page from one exact target after a global sequence.
    pub async fn read_messages(
        &self,
        actor_id: &str,
        target_id: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        self.assert_open()?;
        if after_seq < 0 {
            return Err(CollabError::InvalidArgument(
                "after_seq must not be negative".into(),
            ));
        }
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let connection = self.connection.lock().await;
        require_target_access(&connection, target_id, actor_id).await?;
        let mut rows = connection
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM messages
                 WHERE target_id = ?1 AND seq > ?2
                 ORDER BY seq
                 LIMIT ?3",
                (target_id, after_seq, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(message_from_row(&row)?);
        }
        Ok(messages)
    }

    /// Read the exact total count and the latest `limit` messages (ascending)
    /// of one exact target under a single locked snapshot. Unlike an
    /// `after_seq = 0` page, `count` is never a lower bound and the returned
    /// messages are always the true tail.
    pub async fn read_messages_tail(
        &self,
        actor_id: &str,
        target_id: &str,
        limit: u32,
    ) -> Result<MessageTail> {
        self.assert_open()?;
        if limit == 0 || limit > 100 {
            return Err(CollabError::InvalidArgument(
                "limit must be between 1 and 100".into(),
            ));
        }
        let mut connection = self.connection.lock().await;
        // Explicit read transaction: COUNT and the tail page observe one DB
        // snapshot even when a second connection writes concurrently.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .await?;
        require_target_access(&transaction, target_id, actor_id).await?;
        let mut count_rows = transaction
            .query(
                "SELECT COUNT(*) FROM messages WHERE target_id = ?1",
                (target_id,),
            )
            .await?;
        let count_row = count_rows
            .next()
            .await?
            .ok_or_else(|| CollabError::Database("count returned no row".into()))?;
        let count: i64 = count_row.get(0)?;
        drop(count_rows);
        let mut rows = transaction
            .query(
                "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                 FROM (
                     SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
                     FROM messages
                     WHERE target_id = ?1
                     ORDER BY seq DESC
                     LIMIT ?2
                 )
                 ORDER BY seq",
                (target_id, i64::from(limit)),
            )
            .await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(message_from_row(&row)?);
        }
        drop(rows);
        transaction.commit().await?;
        Ok(MessageTail { count, messages })
    }
    pub(super) async fn send_message_inner(
        &self,
        request: SendMessageRequest,
        _failpoint: SendFailpoint,
    ) -> Result<SendMessageResult> {
        self.assert_open()?;
        for (name, value) in [
            ("target_id", request.target_id.as_str()),
            ("author_id", request.author_id.as_str()),
            ("client_request_id", request.client_request_id.as_str()),
            ("text", request.text.as_str()),
        ] {
            require_non_empty(name, value)?;
        }

        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let route =
            require_target_access(&transaction, &request.target_id, &request.author_id).await?;

        if let Some(message) =
            find_message_by_request(&transaction, &request.author_id, &request.client_request_id)
                .await?
        {
            let (recipient_ids, wake_agent_ids) =
                message_recipients(&transaction, &message.id).await?;
            transaction.commit().await?;
            return Ok(SendMessageResult {
                message,
                recipient_ids,
                wake_agent_ids,
                replayed: true,
            });
        }

        if route.kind == TargetKind::Thread {
            ThreadStore::new(&transaction)
                .ensure_following(&request.target_id, &request.author_id, now)
                .await?;
        }

        let message_id = new_id();
        let body_json = serde_json::to_string(&StoredTextBody {
            kind: "text".into(),
            text: request.text.clone(),
        })
        .map_err(|error| CollabError::InvalidArgument(error.to_string()))?;
        let mut rows = transaction
            .query(
                "INSERT INTO messages
                 (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 RETURNING seq",
                (
                    message_id.as_str(),
                    request.target_id.as_str(),
                    request.author_id.as_str(),
                    request.client_request_id.as_str(),
                    body_json.as_str(),
                    now,
                ),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(CollabError::Database(
                "message insert returned no sequence".into(),
            ));
        };
        let message_seq = row.get::<i64>(0)?;
        drop(rows);

        #[cfg(test)]
        if _failpoint == SendFailpoint::AfterMessageInsert {
            return Err(CollabError::InjectedSendFailure);
        }

        let (recipient_statement, permission_target_id) = if route.kind == TargetKind::Thread {
            (
                "SELECT a.id, a.kind
                 FROM thread_follows f
                 JOIN actors a ON a.id = f.actor_id
                 JOIN memberships m
                   ON m.target_id = ?3 AND m.actor_id = f.actor_id
                 WHERE f.thread_target_id = ?1
                   AND f.unfollowed_at_ms IS NULL
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                route.permission_target_id(&request.target_id),
            )
        } else {
            (
                "SELECT a.id, a.kind
                 FROM memberships m
                 JOIN actors a ON a.id = m.actor_id
                 WHERE m.target_id = ?1
                   AND m.left_at_ms IS NULL
                   AND a.id <> ?2
                 ORDER BY a.id",
                request.target_id.as_str(),
            )
        };
        let mut recipient_rows = if route.kind == TargetKind::Thread {
            transaction
                .query(
                    recipient_statement,
                    (
                        request.target_id.as_str(),
                        request.author_id.as_str(),
                        permission_target_id,
                    ),
                )
                .await?
        } else {
            transaction
                .query(
                    recipient_statement,
                    (request.target_id.as_str(), request.author_id.as_str()),
                )
                .await?
        };
        let mut recipients = Vec::new();
        while let Some(row) = recipient_rows.next().await? {
            recipients.push((row.get::<String>(0)?, row.get::<String>(1)?));
        }
        drop(recipient_rows);

        let mut recipient_ids = Vec::with_capacity(recipients.len());
        let mut wake_agent_ids = Vec::new();
        for (recipient_id, recipient_kind) in recipients {
            let delivery_id = new_id();
            transaction
                .execute(
                    "INSERT INTO deliveries
                     (id, message_id, message_seq, target_id, recipient_id, committed_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        delivery_id.as_str(),
                        message_id.as_str(),
                        message_seq,
                        request.target_id.as_str(),
                        recipient_id.as_str(),
                        now,
                    ),
                )
                .await?;
            if recipient_kind == ActorKind::Agent.as_str() {
                transaction
                    .execute(
                        "INSERT INTO agent_wake_state
                         (agent_id, pending_seq, notified_seq, notified_generation, attempt_count)
                         VALUES (?1, ?2, 0, 0, 0)
                         ON CONFLICT(agent_id) DO UPDATE SET
                           pending_seq = MAX(agent_wake_state.pending_seq, excluded.pending_seq)",
                        (recipient_id.as_str(), message_seq),
                    )
                    .await?;
                wake_agent_ids.push(recipient_id.clone());
            }
            recipient_ids.push(recipient_id);
        }
        insert_target_change(
            &transaction,
            ChangeKind::MessageCreated,
            &request.target_id,
            &message_id,
            &[request.author_id.as_str()],
            now,
        )
        .await?;
        transaction.commit().await?;

        Ok(SendMessageResult {
            message: Message {
                seq: message_seq,
                id: message_id,
                target_id: request.target_id,
                author_id: request.author_id,
                client_request_id: request.client_request_id,
                text: request.text,
                created_at_ms: now,
            },
            recipient_ids,
            wake_agent_ids,
            replayed: false,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct StoredTextBody {
    pub(crate) kind: String,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendFailpoint {
    None,
    #[cfg(test)]
    AfterMessageInsert,
}

pub(crate) fn message_from_row(row: &Row) -> Result<Message> {
    let body_json = row.get::<String>(5)?;
    let body: StoredTextBody = serde_json::from_str(&body_json)
        .map_err(|error| CollabError::Database(format!("message body is malformed: {error}")))?;
    Ok(Message {
        seq: row.get(0)?,
        id: row.get(1)?,
        target_id: row.get(2)?,
        author_id: row.get(3)?,
        client_request_id: row.get(4)?,
        text: body.text,
        created_at_ms: row.get(6)?,
    })
}

pub(crate) fn stored_text(body_json: &str, context: &str) -> Result<String> {
    serde_json::from_str::<StoredTextBody>(body_json)
        .map(|body| body.text)
        .map_err(|error| CollabError::Database(format!("{context} is malformed: {error}")))
}

pub(crate) async fn find_message_by_request(
    connection: &Connection,
    author_id: &str,
    client_request_id: &str,
) -> Result<Option<Message>> {
    let mut rows = connection
        .query(
            "SELECT seq, id, target_id, author_id, client_request_id, body_json, created_at_ms
             FROM messages WHERE author_id = ?1 AND client_request_id = ?2",
            (author_id, client_request_id),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let body_json = row.get::<String>(5)?;
    let body: StoredTextBody = serde_json::from_str(&body_json)
        .map_err(|error| CollabError::Database(format!("message body is malformed: {error}")))?;
    Ok(Some(Message {
        seq: row.get(0)?,
        id: row.get(1)?,
        target_id: row.get(2)?,
        author_id: row.get(3)?,
        client_request_id: row.get(4)?,
        text: body.text,
        created_at_ms: row.get(6)?,
    }))
}

pub(crate) async fn message_recipients(
    connection: &Connection,
    message_id: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut rows = connection
        .query(
            "SELECT d.recipient_id, a.kind
             FROM deliveries d JOIN actors a ON a.id = d.recipient_id
             WHERE d.message_id = ?1 ORDER BY d.recipient_id",
            [message_id],
        )
        .await?;
    let mut recipients = Vec::new();
    let mut agents = Vec::new();
    while let Some(row) = rows.next().await? {
        let recipient = row.get::<String>(0)?;
        if row.get::<String>(1)? == ActorKind::Agent.as_str() {
            agents.push(recipient.clone());
        }
        recipients.push(recipient);
    }
    Ok((recipients, agents))
}

pub(crate) async fn message_target(connection: &Connection, message_id: &str) -> Result<String> {
    let mut rows = connection
        .query("SELECT target_id FROM messages WHERE id = ?1", [message_id])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(row.get(0)?),
        None => Err(not_found("message", message_id)),
    }
}

pub(crate) async fn message_body_text(
    connection: &Connection,
    message_id: &str,
) -> Result<Option<String>> {
    let mut rows = connection
        .query("SELECT body_json FROM messages WHERE id = ?1", [message_id])
        .await?;
    match rows.next().await? {
        Some(row) => {
            let body_json = row.get::<String>(0)?;
            let body: StoredTextBody = serde_json::from_str(&body_json).map_err(|error| {
                CollabError::Database(format!("message '{message_id}' has invalid body: {error}"))
            })?;
            Ok(Some(body.text))
        }
        None => Ok(None),
    }
}

pub(crate) async fn message_target_author(
    connection: &Connection,
    message_id: &str,
) -> Result<(String, String)> {
    let mut rows = connection
        .query(
            "SELECT target_id, author_id FROM messages WHERE id = ?1",
            [message_id],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok((row.get(0)?, row.get(1)?)),
        None => Err(not_found("message", message_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn send_commits_message_deliveries_and_wakes_together() -> Result<()> {
        let (core, user, alpha, beta, channel) = fixture().await?;
        let result = core
            .send_message(SendMessageRequest {
                target_id: channel.id,
                author_id: user.id,
                client_request_id: "send-1".into(),
                text: "review this".into(),
            })
            .await?;

        assert_eq!(
            result.recipient_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert_eq!(
            result.wake_agent_ids,
            vec![alpha.id.clone(), beta.id.clone()]
        );
        assert!(!result.replayed);

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 1);
        assert_eq!(count(&connection, "deliveries").await?, 2);
        assert_eq!(count(&connection, "agent_wake_state").await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn send_rolls_back_message_when_recipient_phase_fails() -> Result<()> {
        let (core, user, _alpha, _beta, channel) = fixture().await?;
        let failure = core
            .send_message_inner(
                SendMessageRequest {
                    target_id: channel.id,
                    author_id: user.id,
                    client_request_id: "send-fail".into(),
                    text: "must roll back".into(),
                },
                SendFailpoint::AfterMessageInsert,
            )
            .await;
        assert!(matches!(failure, Err(CollabError::InjectedSendFailure)));

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 0);
        assert_eq!(count(&connection, "deliveries").await?, 0);
        assert_eq!(count(&connection, "agent_wake_state").await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn repeated_send_request_is_idempotent() -> Result<()> {
        let (core, user, _alpha, _beta, channel) = fixture().await?;
        let request = SendMessageRequest {
            target_id: channel.id,
            author_id: user.id,
            client_request_id: "same-request".into(),
            text: "only once".into(),
        };
        let first = core.send_message(request.clone()).await?;
        let replay = core.send_message(request).await?;
        assert_eq!(first.message, replay.message);
        assert!(replay.replayed);

        let connection = core.connection.lock().await;
        assert_eq!(count(&connection, "messages").await?, 1);
        assert_eq!(count(&connection, "deliveries").await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn exact_target_reads_recheck_current_membership() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let outsider = core.create_user("outsider", "Outsider").await?;
        let sent = core
            .send_message(SendMessageRequest {
                target_id: channel.id.clone(),
                author_id: user.id.clone(),
                client_request_id: "history-message".into(),
                text: "history".into(),
            })
            .await?;

        assert_eq!(
            core.read_message(&alpha.id, &channel.id, &sent.message.id)
                .await?,
            sent.message
        );
        assert_eq!(
            core.read_messages(&alpha.id, &channel.id, 0, 10).await?,
            vec![sent.message.clone()]
        );
        assert!(matches!(
            core.read_message(&outsider.id, &channel.id, &sent.message.id)
                .await,
            Err(CollabError::PermissionDenied { .. })
        ));

        let other = core.create_channel("other", &user.id).await?;
        core.add_member(&other.id, &alpha.id, &user.id).await?;
        assert!(matches!(
            core.read_message(&alpha.id, &other.id, &sent.message.id)
                .await,
            Err(CollabError::NotFound { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn read_messages_tail_returns_exact_count_and_true_tail() -> Result<()> {
        let (core, user, alpha, _beta, channel) = fixture().await?;
        let outsider = core.create_user("tail-outsider", "Tail Outsider").await?;
        let mut sent = Vec::new();
        for index in 0..5 {
            sent.push(
                core.send_message(SendMessageRequest {
                    target_id: channel.id.clone(),
                    author_id: user.id.clone(),
                    client_request_id: format!("tail-message-{index}"),
                    text: format!("tail {index}"),
                })
                .await?
                .message,
            );
        }

        // A small limit still reports the exact total and the true latest page.
        let tail = core.read_messages_tail(&alpha.id, &channel.id, 2).await?;
        assert_eq!(tail.count, 5);
        assert_eq!(tail.messages, sent[3..].to_vec());
        // A limit above the total returns everything, ascending.
        let full = core.read_messages_tail(&alpha.id, &channel.id, 100).await?;
        assert_eq!(full.count, 5);
        assert_eq!(full.messages, sent);
        // An empty target reports zero with no messages.
        let empty = core.create_channel("tail-empty", &user.id).await?;
        core.add_member(&empty.id, &alpha.id, &user.id).await?;
        let empty_tail = core.read_messages_tail(&alpha.id, &empty.id, 10).await?;
        assert_eq!(empty_tail.count, 0);
        assert!(empty_tail.messages.is_empty());
        // Membership and argument validation match the paged read.
        assert!(matches!(
            core.read_messages_tail(&outsider.id, &channel.id, 2).await,
            Err(CollabError::PermissionDenied { .. })
        ));
        assert!(matches!(
            core.read_messages_tail(&alpha.id, &channel.id, 0).await,
            Err(CollabError::InvalidArgument(_))
        ));
        assert!(matches!(
            core.read_messages_tail(&alpha.id, &channel.id, 101).await,
            Err(CollabError::InvalidArgument(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn read_messages_tail_holds_one_snapshot_under_concurrent_writes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tail-snapshot.db");
        let reader = CollabCore::open(&path).await?;
        let user = reader.create_user("owner", "Owner").await?;
        let channel = reader.create_channel("busy", &user.id).await?;
        let writer = CollabCore::open(&path).await?;

        let writer_user = writer.ensure_user("owner", "Owner").await?;
        let write_target = channel.id.clone();

        let writer_task = tokio::spawn(async move {
            for index in 0..30 {
                writer
                    .send_message(SendMessageRequest {
                        target_id: write_target.clone(),
                        author_id: writer_user.id.clone(),
                        client_request_id: format!("concurrent-{index}"),
                        text: format!("concurrent {index}"),
                    })
                    .await?;
                tokio::task::yield_now().await;
            }
            Ok::<(), CollabError>(())
        });

        // Every read must be self-consistent: the page is the contiguous
        // suffix implied by the count of the same snapshot.
        for _ in 0..30 {
            let tail = reader.read_messages_tail(&user.id, &channel.id, 5).await?;
            let page_len = i64::try_from(tail.messages.len()).unwrap();
            assert!(page_len <= tail.count);
            if page_len > 0 {
                let first_seq = tail.messages[0].seq;
                assert_eq!(first_seq, tail.count - page_len + 1);
                for (offset, message) in tail.messages.iter().enumerate() {
                    assert_eq!(message.seq, first_seq + i64::try_from(offset).unwrap());
                }
            }
            tokio::task::yield_now().await;
        }
        writer_task.await.unwrap()?;
        Ok(())
    }
}
