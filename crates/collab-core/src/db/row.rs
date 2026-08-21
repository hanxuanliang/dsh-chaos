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
}

pub(crate) fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}
