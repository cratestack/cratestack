//! Dialect-specific SQL string builders for the Postgres source.
//!
//! Pure functions: no I/O, no schema lookups. Wraps each projection in
//! `row_to_json(t.*)` so the fetch path can stay blind to per-column
//! Postgres types.

use crate::data::model_info::{ModelSqlInfo, PkCast};

fn projection(info: &ModelSqlInfo<'_>) -> String {
    info.columns
        .iter()
        .map(|c| format!("\"{}\"", c.column_name))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn build_list_sql(info: &ModelSqlInfo<'_>, limit: u32) -> String {
    let projection = projection(info);
    let pk = &info.pk_column;
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("($1::text IS NULL OR \"{pk}\" > $1)"),
        PkCast::BigInt => format!("($1::text IS NULL OR \"{pk}\" > $1::bigint)"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {cursor_predicate} \
           ORDER BY \"{pk}\" ASC \
           LIMIT {limit} \
         ) t",
        table = info.table,
    )
}

pub(super) fn build_get_sql(info: &ModelSqlInfo<'_>) -> String {
    let projection = projection(info);
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = $1"),
        PkCast::BigInt => format!("\"{pk}\" = $1::bigint"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {pk_predicate} \
           LIMIT 1 \
         ) t",
        table = info.table,
    )
}

pub(super) fn build_list_on_column_sql(
    info: &ModelSqlInfo<'_>,
    filter_column: &str,
    filter_cast: PkCast,
    limit: u32,
) -> String {
    let projection = projection(info);
    let pk = &info.pk_column;
    let filter_predicate = match filter_cast {
        PkCast::Text => format!("\"{filter_column}\" = $1"),
        PkCast::BigInt => format!("\"{filter_column}\" = $1::bigint"),
    };
    let cursor_predicate = match info.pk_cast {
        PkCast::Text => format!("($2::text IS NULL OR \"{pk}\" > $2)"),
        PkCast::BigInt => format!("($2::text IS NULL OR \"{pk}\" > $2::bigint)"),
    };
    format!(
        "SELECT row_to_json(t.*) AS row \
         FROM ( \
           SELECT {projection} \
           FROM \"{table}\" \
           WHERE {filter_predicate} AND {cursor_predicate} \
           ORDER BY \"{pk}\" ASC \
           LIMIT {limit} \
         ) t",
        table = info.table,
    )
}

pub(super) fn build_insert_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=columns.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let projection = projection(info);
    format!(
        "WITH inserted AS ( \
           INSERT INTO \"{table}\" ({names}) VALUES ({placeholders}) RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM inserted) t",
        table = info.table,
    )
}

pub(super) fn build_update_sql(info: &ModelSqlInfo<'_>, columns: &[String]) -> String {
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let pk_placeholder = columns.len() + 1;
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = ${pk_placeholder}"),
        PkCast::BigInt => format!("\"{pk}\" = ${pk_placeholder}::bigint"),
    };
    let projection = projection(info);
    format!(
        "WITH updated AS ( \
           UPDATE \"{table}\" SET {assignments} WHERE {pk_predicate} RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM updated) t",
        table = info.table,
    )
}

pub(super) fn build_delete_sql(info: &ModelSqlInfo<'_>) -> String {
    let pk = &info.pk_column;
    let pk_predicate = match info.pk_cast {
        PkCast::Text => format!("\"{pk}\" = $1"),
        PkCast::BigInt => format!("\"{pk}\" = $1::bigint"),
    };
    let projection = projection(info);
    format!(
        "WITH deleted AS ( \
           DELETE FROM \"{table}\" WHERE {pk_predicate} RETURNING * \
         ) \
         SELECT row_to_json(t.*) AS row FROM (SELECT {projection} FROM deleted) t",
        table = info.table,
    )
}
