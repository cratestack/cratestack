//! Dialect-specific SQL string builders for the Postgres source.
//!
//! Pure functions: no I/O, no schema lookups. Wraps each projection in
//! `row_to_json(t.*)` so the fetch path can stay blind to per-column
//! Postgres types.

use crate::data::model_info::{ModelSqlInfo, PkCast};

/// Project every column **aliased back to its `.cstack` field name**.
///
/// This alias is not cosmetic. [`crate::data::Row`] documents that row
/// keys are field names as declared in the model, and every consumer
/// relies on it: the UI looks up `row[field.name]`,
/// [`crate::data::common::next_cursor`] extracts the cursor by PK field
/// name, `relations::extract_filter_value` reads the FK by field name,
/// and the audit log reads the new PK by field name. The SQLite source
/// has always honoured that contract (its `json_object(...)` labels are
/// field names); Postgres did not, because `row_to_json(t.*)` keys the
/// object by whatever the subquery called its columns — i.e. the
/// snake_cased *column* names.
///
/// The two only coincide for single-word fields (`id`, `status`), which
/// is why the mismatch stayed invisible: on a schema with `subjectId`
/// or `createdAt` every one of those lookups missed, blanking cells,
/// stalling pagination, breaking relation follow, and — worst — making
/// the drawer's edit form read `""` and write `null` back over
/// untouched optional columns.
fn projection(info: &ModelSqlInfo<'_>) -> String {
    info.columns
        .iter()
        .map(|c| format!("\"{}\" AS \"{}\"", c.column_name, c.field_name))
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

/// `version_column`, when the model declares `@version`, is appended to
/// the `SET` list as `"col" = "col" + 1` — a raw fragment, not a bound
/// placeholder, so it never disturbs the positional `$N` indices the
/// payload columns and the trailing PK bind rely on. Always applied when
/// present; the caller (`ops::update`) only reaches this builder once
/// the write-mode guard has decided the write is safe to route for real
/// (cratestack#507's "option 3") — an unroutable `@version` write is
/// refused before any SQL is built at all.
pub(super) fn build_update_sql(
    info: &ModelSqlInfo<'_>,
    columns: &[String],
    version_column: Option<&str>,
) -> String {
    let mut assignments = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(v) = version_column {
        if !assignments.is_empty() {
            assignments.push_str(", ");
        }
        assignments.push_str(&format!("\"{v}\" = \"{v}\" + 1"));
    }
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
