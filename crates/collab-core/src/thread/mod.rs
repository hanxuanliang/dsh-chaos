//! Thread creation, attention subscriptions, and read projections.

mod model;
pub(crate) mod store;

pub(crate) use model::ThreadId;
pub use model::ThreadSummary;

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::membership::{Membership, MembershipStore};
use crate::message::store::MessageStore;
use crate::target::require_target;
use crate::{
    Actor, ChangeKind, CollabCore, CollabError, Result, Target, TargetKind, new_id, now_ms,
};

use model::{FollowOutcome, RootMessageIds, ThreadAccess, ThreadSubscription};
use store::ThreadStore;

impl CollabCore {
    /// Return the one Thread target rooted at a top-level Message. The creator
    /// and root author follow it immediately when they retain parent access.
    pub async fn create_thread(&self, root_message_id: &str, actor_id: &str) -> Result<Target> {
        require_non_blank!(root_message_id);
        let actor_id = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &actor_id).await?;
            let (parent_target_id, root_author_id) = MessageStore::new(connection)
                .target_and_author(root_message_id)
                .await?;
            if require_target(connection, &parent_target_id).await? == TargetKind::Thread {
                return Err(CollabError::InvalidArgument(
                    "Threads cannot be nested under Thread messages".into(),
                ));
            }
            Membership::require(connection, &parent_target_id, &actor).await?;

            let store = ThreadStore::new(connection);
            if let Some(target) = store.find_by_root(root_message_id).await? {
                return Ok(target);
            }

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
            connection
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
            store.ensure_following(&thread_id, &actor_id, now).await?;
            let root_author_id = ActorId::parse(&root_author_id)?;
            if root_author_id != actor_id
                && MembershipStore::new(connection)
                    .is_active_member(&parent_target_id, root_author_id.as_str())
                    .await?
            {
                store.ensure_following(&thread_id, &root_author_id, now).await?;
            }
            let parent_actor_ids =
                MembershipStore::new(connection).active_member_ids(&parent_target_id).await?;
            ChangeStore::new(connection)
                .insert_change(
                    ChangeKind::TargetCreated,
                    Some(thread_id.as_str()),
                    thread_id.as_str(),
                    &parent_actor_ids,
                    now,
                )
                .await?;
            Ok(target)
        })
        .await
    }

    /// Follow one Thread after rechecking access to its parent target.
    pub async fn follow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.apply_subscription(thread_target_id, actor_id, ThreadSubscription::follow)
            .await
    }

    /// Stop future ordinary Thread delivery for one current parent member.
    pub async fn unfollow_thread(&self, thread_target_id: &str, actor_id: &str) -> Result<()> {
        self.apply_subscription(thread_target_id, actor_id, ThreadSubscription::unfollow)
            .await
    }

    /// The fixed write pipeline for one subscription transition: load the
    /// proven scope, decide the state move, persist, and publish only when
    /// the state actually moved.
    async fn apply_subscription(
        &self,
        thread_target_id: &str,
        actor_id: &str,
        transition: fn(&mut ThreadSubscription) -> FollowOutcome,
    ) -> Result<()> {
        let thread_id = ThreadId::parse(thread_target_id)?;
        let actor_id = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ThreadStore::new(connection);
            let scope = ThreadAccess::require(connection, &actor_id, &thread_id).await?;
            let mut subscription = store.load_subscription(&thread_id, &actor_id).await?;
            let outcome = transition(&mut subscription);
            store.save_subscription(&subscription, outcome, now).await?;
            if outcome.changed() {
                let recipients = MembershipStore::new(connection)
                    .active_member_ids(&scope.permission_target_id)
                    .await?;
                ChangeStore::new(connection)
                    .insert_change(
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

impl CollabCore {
    /// Return reply counts and the three most recent distinct repliers for up
    /// to 100 root Messages. Inaccessible and empty Threads are omitted.
    pub async fn thread_summaries(
        &self,
        actor_id: &str,
        root_message_ids: &[String],
    ) -> Result<Vec<ThreadSummary>> {
        let actor_id = ActorId::parse(actor_id)?;
        let roots = RootMessageIds::parse(root_message_ids, 100)?;
        self.read(async |connection| {
            ThreadStore::new(connection)
                .summaries(&actor_id, &roots)
                .await
        })
        .await
    }
}

// ── 能力（域公开函数） ─────────────────────────────────────────────────────────

pub(crate) async fn followed_thread_ids_for_actor(
    connection: &turso::Connection,
    actor_id: &str,
) -> Result<Vec<String>> {
    let actor_id = ActorId::parse(actor_id)?;
    ThreadStore::new(connection)
        .followed_thread_ids(&actor_id)
        .await
}
