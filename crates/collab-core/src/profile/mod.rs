//! Stable Agent identity, Profile, Charter, and lifecycle operations.

mod model;
pub(crate) mod store;

pub use model::{AgentCharter, AgentLifecycle, AgentProfile};

use crate::actor::ActorId;
use crate::changefeed::ChangeStore;
use crate::membership::MembershipStore;
use crate::{
    Actor, ActorKind, ChangeKind, CollabCore, CollabError, IdentityContext, NonBlank, Result,
    now_ms,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use store::ProfileStore;

const AGENT_AVATAR_MAX_BYTES: usize = 256 * 1024;
/// Legacy default handle from pre-OS-username builds. ensure_user adopts
/// (rehandles) this row onto the OS-username slug instead of splitting the
/// local user into two actors.
const LEGACY_LOCAL_USER_HANDLE: &str = "local-user";

fn normalize_avatar_data_url(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    let (mime, encoded) = ["image/png", "image/jpeg", "image/webp"]
        .into_iter()
        .find_map(|mime| {
            value
                .strip_prefix(&format!("data:{mime};base64,"))
                .map(|data| (mime, data))
        })
        .ok_or_else(|| {
            CollabError::InvalidArgument(
                "avatar must be a base64 PNG, JPEG, or WebP data URL".into(),
            )
        })?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| CollabError::InvalidArgument("avatar contains invalid base64 data".into()))?;
    if bytes.is_empty() || bytes.len() > AGENT_AVATAR_MAX_BYTES {
        return Err(CollabError::InvalidArgument(format!(
            "avatar must contain 1..={AGENT_AVATAR_MAX_BYTES} bytes"
        )));
    }
    let signature_matches = match mime {
        "image/png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !signature_matches {
        return Err(CollabError::InvalidArgument(format!(
            "avatar bytes do not match declared MIME type '{mime}'"
        )));
    }
    Ok(Some(format!(
        "data:{mime};base64,{}",
        STANDARD.encode(bytes)
    )))
}

impl CollabCore {
    /// Create the one local human/user actor.
    pub async fn create_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        let now = now_ms()?;
        self.write(async |connection| {
            Actor::insert(connection, ActorKind::User, handle, display_name, now).await
        })
        .await
    }

    /// Create a stable Agent actor and record its durable workspace path.
    pub async fn create_agent(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
    ) -> Result<Actor> {
        self.create_agent_actor(
            handle,
            display_name,
            workspace_path,
            &AgentCharter::default(),
        )
        .await
    }

    /// Create a stable Agent with its first versioned Charter.
    pub async fn create_agent_profile(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
        charter: AgentCharter,
    ) -> Result<AgentProfile> {
        let charter = charter.normalize()?;
        let actor = self
            .create_agent_actor(handle, display_name, workspace_path, &charter)
            .await?;
        self.agent_profile(&actor.id).await
    }

    /// Shared Agent creation: one Actor, its agents row, and the ActorCreated
    /// change, all committed in one transaction.
    async fn create_agent_actor(
        &self,
        handle: &str,
        display_name: &str,
        workspace_path: &str,
        charter: &AgentCharter,
    ) -> Result<Actor> {
        NonBlank::parse("workspace_path", workspace_path)?;
        let charter_json = charter.encode()?;
        let now = now_ms()?;
        self.write(async |connection| {
            let actor =
                Actor::insert(connection, ActorKind::Agent, handle, display_name, now).await?;
            let workspace_path = workspace_path.replace("{id}", actor.id.as_str());
            ProfileStore::new(connection)
                .insert_agent(&actor.id, &workspace_path, &charter_json, now)
                .await?;
            Ok(actor)
        })
        .await
    }

    /// Replace the mutable display name and Charter under an optimistic Profile fence.
    pub async fn update_agent_profile(
        &self,
        agent_id: &str,
        display_name: &str,
        charter: AgentCharter,
        expected_version: i64,
    ) -> Result<AgentProfile> {
        if expected_version <= 0 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let display_name = display_name.trim();
        let charter = charter.normalize()?;
        let charter_json = charter.encode()?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ProfileStore::new(connection);
            let current = store.require_profile(agent_id).await?;
            if current.version != expected_version {
                return Err(CollabError::AgentProfileVersionConflict {
                    agent_id: agent_id.to_owned(),
                    expected: expected_version,
                    actual: current.version,
                });
            }
            let next_version = current.version + 1;
            crate::actor::ActorStore::new(connection)
                .rename(&ActorId::parse(agent_id)?, display_name)
                .await?;
            store
                .update_charter(agent_id, &charter_json, next_version, now)
                .await?;
            let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
            ChangeStore::new(connection)
                .insert_change(
                    ChangeKind::AgentProfileChanged,
                    None,
                    agent_id,
                    &actor_ids,
                    now,
                )
                .await?;
            store.require_profile(agent_id).await
        })
        .await
    }

    /// Replace or clear one Agent's custom avatar under the same Profile fence.
    pub async fn update_agent_avatar(
        &self,
        agent_id: &str,
        avatar_data_url: Option<&str>,
        expected_version: i64,
    ) -> Result<AgentProfile> {
        if expected_version <= 0 {
            return Err(CollabError::InvalidArgument(
                "expected_version must be positive".into(),
            ));
        }
        let avatar_data_url = normalize_avatar_data_url(avatar_data_url)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let store = ProfileStore::new(connection);
            let current = store.require_profile(agent_id).await?;
            if current.version != expected_version {
                return Err(CollabError::AgentProfileVersionConflict {
                    agent_id: agent_id.to_owned(),
                    expected: expected_version,
                    actual: current.version,
                });
            }
            let next_version = current.version + 1;
            crate::actor::ActorStore::new(connection)
                .update_avatar_data_url(&ActorId::parse(agent_id)?, avatar_data_url.as_deref())
                .await?;
            store.advance_version(agent_id, next_version, now).await?;
            let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
            ChangeStore::new(connection)
                .insert_change(
                    ChangeKind::AgentProfileChanged,
                    None,
                    agent_id,
                    &actor_ids,
                    now,
                )
                .await?;
            store.require_profile(agent_id).await
        })
        .await
    }

    /// Delete one Agent's operational state. The actor row and its messages
    /// stay so history never points at a missing author.
    pub async fn delete_agent(&self, actor_id: &str) -> Result<()> {
        let _ = ActorId::parse(actor_id)?;
        let now = now_ms()?;
        self.write(async |connection| {
            let actor = Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            if actor.kind != ActorKind::Agent {
                return Err(CollabError::NotFound {
                    entity: "agent",
                    id: actor_id.to_owned(),
                });
            }
            ProfileStore::new(connection)
                .delete_operational_state(actor_id)
                .await?;
            // The actor set changed; reuse actor_created so clients re-pull actors.
            let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
            ChangeStore::new(connection)
                .insert_change(ChangeKind::ActorCreated, None, &actor.id, &actor_ids, now)
                .await?;
            Ok(())
        })
        .await
    }

    /// Return the stable User for one handle, creating it when absent.
    ///
    /// Legacy-handle adoption (owner ruling: the local user is fixed on this
    /// machine, so the handle reads as the OS username): when the requested
    /// handle does not exist but the legacy 'local-user' User does (and no
    /// Agent squatted the new handle), that row is ADOPTED — one UPDATE moves
    /// memberships, tasks, follows, and history to the new handle; no second
    /// User row is created.
    pub async fn ensure_user(&self, handle: &str, display_name: &str) -> Result<Actor> {
        NonBlank::parse("handle", handle)?;
        NonBlank::parse("display_name", display_name)?;
        let now = now_ms()?;
        self.write(async |connection| {
            if let Some(actor) = Actor::find_by_handle(connection, handle).await? {
                if actor.kind != ActorKind::User {
                    return Err(CollabError::InvalidArgument(format!(
                        "actor handle '{handle}' belongs to an Agent"
                    )));
                }
                if actor.display_name != display_name {
                    // Only leftover default names migrate. A custom display name
                    // is user-owned and must survive later OS-username ensure.
                    if actor.display_name != "Local User" {
                        return Ok(actor);
                    }
                    // Explicit display-name migration on the stable handle: the
                    // actor id, Memberships and Tasks are untouched.
                    crate::actor::ActorStore::new(connection)
                        .rename(&ActorId::parse(&actor.id)?, display_name)
                        .await?;
                    let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
                    ChangeStore::new(connection)
                        .insert_change(ChangeKind::ActorCreated, None, &actor.id, &actor_ids, now)
                        .await?;
                    return Ok(Actor {
                        display_name: display_name.to_owned(),
                        ..actor
                    });
                }
                return Ok(actor);
            }

            // Legacy 'local-user' adoption: rename the row in place instead of
            // leaving a split identity behind (id, memberships, tasks, and
            // message authorship all keep pointing at this actor id).
            if handle != LEGACY_LOCAL_USER_HANDLE {
                if let Some(legacy) = Actor::find_by_handle(
                    connection,
                    LEGACY_LOCAL_USER_HANDLE,
                )
                .await?
                {
                    if legacy.kind == ActorKind::User {
                        let id = ActorId::parse(&legacy.id)?;
                        crate::actor::ActorStore::new(connection)
                            .rehandle(&id, handle)
                            .await?;
                        if legacy.display_name != display_name
                            && legacy.display_name == "Local User"
                        {
                            crate::actor::ActorStore::new(connection)
                                .rename(&id, display_name)
                                .await?;
                        }
                        let actor_ids = ChangeStore::new(connection).all_actor_ids().await?;
                        ChangeStore::new(connection)
                            .insert_change(
                                ChangeKind::ActorCreated,
                                None,
                                &legacy.id,
                                &actor_ids,
                                now,
                            )
                            .await?;
                        return Ok(Actor {
                            handle: handle.to_owned(),
                            display_name: display_name.to_owned(),
                            ..legacy
                        });
                    }
                }
            }

            Actor::insert(connection, ActorKind::User, handle, display_name, now).await
        })
        .await
    }
}

impl CollabCore {
    /// Read one active stable Agent Profile independently from its DSH Session.
    pub async fn agent_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        NonBlank::parse("agent_id", agent_id)?;
        self.read(async |connection| {
            ProfileStore::new(connection)
                .require_profile(agent_id)
                .await
        })
        .await
    }

    /// List every live Agent Profile in one coherent directory read.
    pub async fn list_agent_profiles(&self, actor_id: &str) -> Result<Vec<AgentProfile>> {
        self.read(async |connection| {
            Actor::require(connection, &ActorId::parse(actor_id)?).await?;
            ProfileStore::new(connection).directory().await
        })
        .await
    }

    /// Return the caller's stable identity and optional exact target roster.
    pub async fn identity_context(
        &self,
        agent_id: &str,
        target_id: Option<&str>,
    ) -> Result<IdentityContext> {
        NonBlank::parse("agent_id", agent_id)?;
        self.read(async |connection| {
            MembershipStore::new(connection)
                .identity_context_for(agent_id, target_id)
                .await
        })
        .await
    }
}
