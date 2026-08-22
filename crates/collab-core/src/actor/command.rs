//! Command-side write composition shared by the profile creation entry points.

use turso::Connection;

use crate::changefeed::{all_actor_ids, insert_change};
use crate::{Actor, ActorKind, ChangeKind, CollabError, Result, new_id};

/// Insert one Actor of `kind` and emit ActorCreated to every known actor.
pub(crate) async fn insert_actor(
    connection: &Connection,
    kind: ActorKind,
    handle: &str,
    display_name: &str,
    now: i64,
) -> Result<Actor> {
    CollabError::require_non_blank("handle", handle)?;
    CollabError::require_non_blank("display_name", display_name)?;
    let actor = Actor {
        id: new_id(),
        kind,
        handle: handle.to_owned(),
        display_name: display_name.to_owned(),
        created_at_ms: now,
    };
    actor.insert(connection).await?;
    let actor_ids = all_actor_ids(connection).await?;
    insert_change(
        connection,
        ChangeKind::ActorCreated,
        None,
        &actor.id,
        &actor_ids,
        now,
    )
    .await?;
    Ok(actor)
}
