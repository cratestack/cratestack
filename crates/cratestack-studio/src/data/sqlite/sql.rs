//! Dialect-specific SQL string builders for the SQLite source.
//!
//! Pure functions: no I/O, no schema lookups. Callers feed in a
//! [`ModelSqlInfo`] (resolved via [`super::super::model_info::resolve_model`])
//! plus any operation-specific parameters and get back a SQL string
//! whose bound parameter slots line up with `?1`, `?2`, … in order.

use crate::data::model_info::{ModelSqlInfo, PkCast};

/// Build a `json_object('field1', "col1", …)` projection that the
/// fetch path round-trips back into a [`crate::data::Row`].
pub(super) fn build_json_object(info: &ModelSqlInfo<'_>) -> String {
    info.columns
        .iter()
        .map(|c| {
            format!(
                "'{name}', \"{col}\"",
                name = sql_quote_string(c.field_name),
                col = c.column_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn build_list_sql(info: &ModelSqlInfo<'_>, limit: u32) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("(?1 IS NULL OR \"{pk}\" > ?1)"),
        PkCast::BigInt => format!("(?1 IS NULL OR \"{pk}\" > CAST(?1 AS INTEGER))"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {cursor_predicate} \
         ORDER BY \"{pk}\" ASC \
         LIMIT {limit}",
        table = info.table,
    )
}

pub(super) fn build_get_sql(info: &ModelSqlInfo<'_>) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?1"),
        PkCast::BigInt => format!("\"{pk}\" = CAST(?1 AS INTEGER)"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {pk_predicate} \
         LIMIT 1",
        table = info.table,
    )
}

pub(super) fn build_list_on_column_sql(
    info: &ModelSqlInfo<'_>,
    filter_column: &str,
    filter_cast: PkCast,
    limit: u32,
) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let filter_predicate = match filter_cast {
        PkCast::Text => format!("\"{filter_column}\" = ?1"),
        PkCast::BigInt => format!("\"{filter_column}\" = CAST(?1 AS INTEGER)"),
    };
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("(?2 IS NULL OR \"{pk}\" > ?2)"),
        PkCast::BigInt => format!("(?2 IS NULL OR \"{pk}\" > CAST(?2 AS INTEGER))"),
    };
    format!(
        "SELECT json_object({object}) AS row \
         FROM \"{table}\" \
         WHERE {filter_predicate} AND {cursor_predicate} \
         ORDER BY \"{pk}\" ASC \
         LIMIT {limit}",
        table = info.table,
    )
}

pub(super) fn build_insert_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let object = build_json_object(info);
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=columns.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{table}\" ({names}) VALUES ({placeholders}) \
         RETURNING json_object({object}) AS row",
        table = info.table,
    )
}

pub(super) fn build_update_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let object = build_json_object(info);
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let pk_placeholder = columns.len() + 1;
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?{pk_placeholder}"),
        PkCast::BigInt => format!("\"{pk}\" = CAST(?{pk_placeholder} AS INTEGER)"),
    };
    format!(
        "UPDATE \"{table}\" SET {assignments} WHERE {pk_predicate} \
         RETURNING json_object({object}) AS row",
        table = info.table,
    )
}

pub(super) fn build_delete_sql(info: &ModelSqlInfo<'_>) -> String {
    let object = build_json_object(info);
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ?1"),
        PkCast::BigInt => format!("\"{pk}\" = CAST(?1 AS INTEGER)"),
    };
    format!(
        "DELETE FROM \"{table}\" WHERE {pk_predicate} \
         RETURNING json_object({object}) AS row",
        table = info.table,
    )
}

/// SQLite single-quote escape for use inside a literal.
fn sql_quote_string(value: &str) -> String {
    value.replace('\'', "''")
}
