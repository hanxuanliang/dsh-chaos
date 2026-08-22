//! Shared row-decoding helpers.
//!
//! Domain types implement [`FromRow`] next to the type. This module only owns
//! the trait, the query methods, and scan helpers.

use turso::{Connection, IntoParams, Row};

use crate::Result;

pub(crate) trait FromRow: Sized {
    fn from_row(row: &Row) -> Result<Self>;
}

pub(crate) trait QueryRows {
    /// Collect every row of a query into decoded values.
    async fn query_rows<T: FromRow>(&self, sql: &str, params: impl IntoParams) -> Result<Vec<T>>;

    /// Decode the first row of a query, yielding `None` when no row exists.
    async fn query_row<T: FromRow>(&self, sql: &str, params: impl IntoParams) -> Result<Option<T>>;

    /// Report whether a query produces at least one row.
    async fn exists(&self, sql: &str, params: impl IntoParams) -> Result<bool>;
}

impl QueryRows for Connection {
    async fn query_rows<T: FromRow>(&self, sql: &str, params: impl IntoParams) -> Result<Vec<T>> {
        let mut rows = self.query(sql, params).await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(T::from_row(&row)?);
        }
        Ok(out)
    }

    async fn query_row<T: FromRow>(&self, sql: &str, params: impl IntoParams) -> Result<Option<T>> {
        let mut rows = self.query(sql, params).await?;
        match rows.next().await? {
            Some(row) => T::from_row(&row).map(Some),
            None => Ok(None),
        }
    }

    async fn exists(&self, sql: &str, params: impl IntoParams) -> Result<bool> {
        let mut rows = self.query(sql, params).await?;
        Ok(rows.next().await?.is_some())
    }
}

impl FromRow for Option<i64> {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(row.get(0)?)
    }
}

impl FromRow for i64 {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(row.get(0)?)
    }
}

impl FromRow for String {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(row.get(0)?)
    }
}

pub(crate) fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fail unless exactly one row was written, naming the operation.
pub(crate) fn assert_one_row(changed: u64, operation: &str) -> crate::Result<()> {
    if changed != 1 {
        return Err(crate::CollabError::Database(format!(
            "{operation} did not update one row"
        )));
    }
    Ok(())
}

/// Fail unless a scalar aggregate query produced a row.
pub(crate) fn require_scalar_row<T>(row: Option<T>, operation: &str) -> crate::Result<T> {
    row.ok_or_else(|| crate::CollabError::Database(format!("{operation} returned no row")))
}
