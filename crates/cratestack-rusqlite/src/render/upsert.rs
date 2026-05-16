//! `INSERT ... ON CONFLICT DO UPDATE` rendering. Mirrors the sqlx path
//! minus the `SELECT FOR UPDATE` probe: no audit, no event outbox, no
//! policies on the embedded layer.

use std::fmt::Write;

use cratestack_sql::{ConflictTarget, Dialect, ModelDescriptor, SqlColumnValue, SqlValue};

/// Render an `INSERT ... ON CONFLICT (<pk>) DO UPDATE SET ... RETURNING ...`
/// upsert. The DO UPDATE clause uses only columns in
/// `descriptor.upsert_update_columns` (PK, `@version`, `@readonly`,
/// `@server_only`, `@default(...)` excluded by the macro). The `@version`
/// column, when present, is incremented via `<table>.<col> + 1` so
/// concurrent upserts converge to the right value.
pub fn render_upsert<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    values: &[SqlColumnValue],
) -> (String, Vec<SqlValue>) {
    render_upsert_with_conflict(dialect, descriptor, values, ConflictTarget::PrimaryKey)
}

/// Render an upsert against an arbitrary conflict target. The default
/// `render_upsert` wraps this with `ConflictTarget::PrimaryKey` so the
/// older public surface stays bit-identical; new callers that need a
/// composite unique key pass `ConflictTarget::Columns(&[..])`.
pub fn render_upsert_with_conflict<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!("INSERT INTO {} (", descriptor.table_name);
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(value.column);
    }
    sql.push_str(") VALUES (");
    let mut binds = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        dialect.write_placeholder(&mut sql, idx + 1);
        binds.push(value.value.clone());
    }
    sql.push_str(") ON CONFLICT (");
    match conflict_target {
        ConflictTarget::PrimaryKey => {
            sql.push_str(descriptor.primary_key);
        }
        ConflictTarget::Columns(cols) => {
            for (idx, column) in cols.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                sql.push_str(column);
            }
        }
    }
    sql.push_str(") DO UPDATE SET ");
    if descriptor.upsert_update_columns.is_empty() {
        // Degenerate case — touch the PK to itself so RETURNING still
        // resolves to the conflicting row. Mirrors the sqlx fallback.
        let _ = write!(
            &mut sql,
            "{pk} = excluded.{pk}",
            pk = descriptor.primary_key,
        );
    } else {
        for (idx, column) in descriptor.upsert_update_columns.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            let _ = write!(&mut sql, "{column} = excluded.{column}");
        }
    }
    if let Some(version_col) = descriptor.version_column {
        let _ = write!(
            &mut sql,
            ", {version_col} = {table}.{version_col} + 1",
            table = descriptor.table_name,
        );
    }
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, binds)
}
