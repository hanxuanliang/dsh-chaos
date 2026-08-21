//! Shared row-decoding helpers.
//!
//! Domain types implement [`FromRow`] next to the type. This module only owns
//! the trait and the scan helpers.

use turso::{Connection, IntoParams, Row, Rows};

use crate::Result;

pub(crate) trait FromRow: Sized {
    fn from_row(row: &Row) -> Result<Self>;
}

pub(crate) async fn query_all<T: FromRow>(
    connection: &Connection,
    sql: &str,
    params: impl IntoParams,
) -> Result<Vec<T>> {
    let mut rows = connection.query(sql, params).await?;
    collect_rows(&mut rows).await
}

async fn collect_rows<T: FromRow>(rows: &mut Rows) -> Result<Vec<T>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(T::from_row(&row)?);
    }
    Ok(out)
}

pub(crate) fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}
