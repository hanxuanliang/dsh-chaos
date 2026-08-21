use turso::transaction::TransactionBehavior;

use crate::changefeed::insert_change;
use crate::ids::{ActorId, ThreadId};
use crate::membership::{Membership, active_member_ids};
use crate::message::message_target_author;
use crate::target::{find_target, is_active_member, require_target};
use crate::{
    Actor, ChangeKind, CollabCore, CollabError, Result, Target, TargetKind, new_id, now_ms,
};

use super::model::ThreadAccess;
use super::store::ThreadStore;

impl CollabCore {
    /// Return the one Thread target rooted at a top-level Message. The creator
    /// and root author follow it immediately when they retain parent access.
    pub async fn create_thread(&self, root_message_id: &str, actor_id: &str) -> Result<Target> {
        self.assert_open()?;
        crate::require_non_empty("root_message_id", root_message_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        let mut connection = self.connection.lock().await;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let actor = Actor::require(&transaction, &actor_id).await?;
        let (parent_target_id, root_author_id) =
            message_target_author(&transaction, root_message_id).await?;
        if require_target(&transaction, &parent_target_id).await? == TargetKind::Thread {
            return Err(CollabError::InvalidArgument(
                "Threads cannot be nested under Thread messages".into(),
            ));
        }
        Membership::require(&transaction, &parent_target_id, &actor).await?;

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

        let thread_id = ThreadId::parse(&new_id())?;
        let target = Target {
            id: thread_id.as_str().to_owned(),
            kind: TargetKind::Thread,
            name: format!("thread:{root_message_id}"),
            parent_target_id: Some(parent_target_id.clone()),
            root_message_id: Some(root_message_id.to_owned()),
            created_by: actor_id.as_str().to_owned(),
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
                    actor_id.as_str(),
                    now,
                ),
            )
            .await?;
        let store = ThreadStore::new(&transaction);
        store
            .ensure_following(thread_id.as_str(), actor_id.as_str(), now)
            .await?;
        if root_author_id != actor_id.as_str()
            && is_active_member(&transaction, &parent_target_id, &root_author_id).await?
        {
            store
                .ensure_following(thread_id.as_str(), &root_author_id, now)
                .await?;
        }
        let parent_actor_ids = active_member_ids(&transaction, &parent_target_id).await?;
        insert_change(
            &transaction,
            ChangeKind::TargetCreated,
            Some(thread_id.as_str()),
            thread_id.as_str(),
            &parent_actor_ids,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(target)
    }

    /// Follow one Thread after rechecking access to its parent target.
    pub async fn follow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        let thread_id = ThreadId::parse(thread_target_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ThreadStore::new(connection);
            let scope = ThreadAccess::require(connection, &actor_id, &thread_id).await?;
            let mut subscription = store.load_subscription(&thread_id, &actor_id).await?;
            let outcome = subscription.follow();
            store.save_subscription(&subscription, outcome, now).await?;
            if outcome.changed() {
                let recipients = active_member_ids(connection, &scope.permission_target_id).await?;
                insert_change(
                    connection,
                    ChangeKind::ThreadFollowChanged,
                    Some(scope.thread_id.as_str()),
                    actor_id.as_str(),
                    &recipients,
                    now,
                )
                .await?;
            }
            Ok(())
        })
        .await
    }

    /// Stop future ordinary Thread delivery for one current parent member.
    pub async fn unfollow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        let thread_id = ThreadId::parse(thread_target_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ThreadStore::new(connection);
            let scope = ThreadAccess::require(connection, &actor_id, &thread_id).await?;
            let mut subscription = store.load_subscription(&thread_id, &actor_id).await?;
            let outcome = subscription.unfollow();
            store.save_subscription(&subscription, outcome, now).await?;
            if outcome.changed() {
                let recipients = active_member_ids(connection, &scope.permission_target_id).await?;
                insert_change(
                    connection,
                    ChangeKind::ThreadFollowChanged,
                    Some(scope.thread_id.as_str()),
                    actor_id.as_str(),
                    &recipients,
                    now,
                )
                .await?;
            }
            Ok(())
        })
        .await
    }
}
