//! Shared row-decoding helpers.
//!
//! Domain types implement [`FromRow`] next to the type. This module only owns
//! the trait and the scan helpers.

use turso::{IntoParams, Row, Rows};

use crate::{not_found, Result};

pub(crate) trait FromRow: Sized {
    fn from_row(row: &Row) -> Result<Self>;
}

pub(crate) async fn query_all<T: FromRow>(
    connection: &turso::Connection,
    sql: &str,
    params: impl IntoParams,
) -> Result<Vec<T>> {
    let mut rows = connection.query(sql, params).await?;
    collect_rows(&mut rows).await
}

pub(crate) async fn query_one<T: FromRow>(
    connection: &turso::Connection,
    sql: &str,
    params: impl IntoParams,
    entity: &'static str,
    id: &str,
) -> Result<T> {
    let mut rows = connection.query(sql, params).await?;
    one_row(&mut rows, entity, id).await
}

pub(crate) async fn query_optional<T: FromRow>(
    connection: &turso::Connection,
    sql: &str,
    params: impl IntoParams,
) -> Result<Option<T>> {
    let mut rows = connection.query(sql, params).await?;
    match rows.next().await? {
        Some(row) => T::from_row(&row).map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn collect_rows<T: FromRow>(rows: &mut Rows) -> Result<Vec<T>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(T::from_row(&row)?);
    }
    Ok(out)
}

pub(crate) async fn one_row<T: FromRow>(
    rows: &mut Rows,
    entity: &'static str,
    id: &str,
) -> Result<T> {
    let Some(row) = rows.next().await? else {
        return Err(not_found(entity, id));
    };
    T::from_row(&row)
}

pub(crate) fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}
