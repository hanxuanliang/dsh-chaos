//! Coarse-grained NAPI bridge for the Rust collab core.

use std::sync::Arc;

use crate::{CollabCore, CollabError};
use napi::{Error, Result, Status};
use napi_derive::napi;

mod activity;
mod actor;
mod changefeed;
mod delivery;
mod membership;
mod message;
mod profile;
mod runtime;
mod target;
mod task;
mod thread;

pub use activity::*;
pub use actor::*;
pub use changefeed::*;
pub use delivery::*;
pub use membership::*;
pub use message::*;
pub use profile::*;
pub use runtime::*;
pub use target::*;
pub use task::*;
pub use thread::*;

#[napi]
pub struct CollabHandle {
    pub(crate) core: Arc<CollabCore>,
}

#[napi]
pub async fn open_collab(path: String) -> Result<CollabHandle> {
    let core = CollabCore::open(path).await.map_err(to_napi_error)?;
    Ok(CollabHandle {
        core: Arc::new(core),
    })
}

#[napi]
impl CollabHandle {
    #[napi]
    pub async fn close(&self) -> Result<()> {
        self.core.close().await.map_err(to_napi_error)
    }
}

pub(crate) fn parse_i64(name: &str, value: &str) -> Result<i64> {
    value.parse().map_err(|_| {
        Error::new(
            Status::InvalidArg,
            format!("[invalid_argument] {name} must be a signed 64-bit decimal string"),
        )
    })
}

pub(crate) fn parse_millis(name: &str, value: f64) -> Result<i64> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_SAFE_INTEGER {
        return Err(Error::new(
            Status::InvalidArg,
            format!("[invalid_argument] {name} must be a non-negative integer millisecond value"),
        ));
    }
    Ok(value as i64)
}

pub(crate) fn to_napi_error(error: CollabError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("[{}] {error}", error.code()),
    )
}
