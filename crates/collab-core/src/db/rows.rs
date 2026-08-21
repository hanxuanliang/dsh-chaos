//! Shared database-row decoders.
//!
//! Domain-specific decoders stay with their domain; this module only owns
//! projections reused by more than one domain.

use turso::Row;

use crate::{Actor, Result, parse_actor_kind};

pub(crate) fn actor_from_row(row: &Row) -> Result<Actor> {
    let id = row.get::<String>(0)?;
    let kind_text = row.get::<String>(1)?;
    Ok(Actor {
        kind: parse_actor_kind(&id, &kind_text)?,
        id,
        handle: row.get(2)?,
        display_name: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}
