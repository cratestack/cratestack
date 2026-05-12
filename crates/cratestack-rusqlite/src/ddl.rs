//! Schema bootstrap for on-device SQLite.
//!
//! Server-side migrations don't apply to mobile — there's no DBA, no
//! downtime, and the app may be installing fresh on a million devices at
//! once. Instead the runtime ensures the table shape exists on app start.
//!
//! [`create_table_sql`] emits a `CREATE TABLE IF NOT EXISTS` statement that
//! matches the columns the macro projects in the descriptor.
//!
//! **Why every column is declared `BLOB`:** SQLite's TEXT affinity converts
//! INTEGER bindings to text-form on write (so `Bool(true)` binds as
//! `INTEGER(1)` but ends up stored as `"1"`, breaking the integer decoder
//! on read). NUMERIC affinity goes the other way and converts numeric-
//! looking TEXT to REAL (the Decimal-precision bug). BLOB affinity is the
//! only one that preserves the storage class of every value we bind — the
//! [`value.rs`](crate::value) module commits to canonical storage classes
//! per `SqlValue` variant, and BLOB respects them. The cost is that integer
//! primary keys don't alias to rowid for auto-increment; production
//! schemas typically use UUID PKs anyway.
//!
//! What this *does not* do: composite indexes, foreign keys, named
//! constraints. Those are app-specific and the runtime exposes
//! [`RusqliteRuntime::with_connection`] so the app can run any
//! additional DDL it needs.

use std::fmt::Write;

use cratestack_sql::ModelDescriptor;

/// Build the `CREATE TABLE IF NOT EXISTS` statement for a descriptor.
///
/// Column types are best-effort: anything we can't infer falls back to
/// SQLite's catch-all `TEXT` affinity, which is the safest default given
/// our binding choices (UUID, DateTime, Decimal, JSON all bind as TEXT).
/// The primary key is marked `PRIMARY KEY` inline.
pub fn create_table_sql<M, PK>(descriptor: &ModelDescriptor<M, PK>) -> String {
    let mut sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n",
        descriptor.table.table_name
    );
    for (idx, column) in descriptor.table.columns.iter().enumerate() {
        if idx > 0 {
            sql.push_str(",\n");
        }
        let _ = write!(&mut sql, "    {} BLOB", column.sql_name);
        if column.sql_name == descriptor.table.primary_key {
            sql.push_str(" PRIMARY KEY");
        }
    }
    if let Some(deleted_at) = descriptor.lifecycle.soft_delete_column {
        if !descriptor
            .table
            .columns
            .iter()
            .any(|c| c.sql_name == deleted_at)
        {
            sql.push_str(",\n    ");
            sql.push_str(deleted_at);
            sql.push_str(" BLOB");
        }
    }
    sql.push_str("\n)");
    sql
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_sql::{
        AuditConfig, AuthPolicies, LifecycleConfig, ModelColumn, QueryCapabilities, TableMeta,
    };

    fn descriptor() -> ModelDescriptor<(), i64> {
        const COLUMNS: &[ModelColumn] = &[
            ModelColumn { rust_name: "id", sql_name: "id" },
            ModelColumn { rust_name: "title", sql_name: "title" },
        ];
        ModelDescriptor::new(
            TableMeta {
                schema_name: "Post",
                table_name: "posts",
                columns: COLUMNS,
                primary_key: "id",
            },
            QueryCapabilities {
                allowed_fields: &[],
                allowed_includes: &[],
                allowed_sorts: &[],
            },
            AuthPolicies {
                read_allow_policies: &[],
                read_deny_policies: &[],
                detail_allow_policies: &[],
                detail_deny_policies: &[],
                create_allow_policies: &[],
                create_deny_policies: &[],
                update_allow_policies: &[],
                update_deny_policies: &[],
                delete_allow_policies: &[],
                delete_deny_policies: &[],
            },
            AuditConfig {
                audit_enabled: false,
                pii_columns: &[],
                sensitive_columns: &[],
            },
            LifecycleConfig {
                create_defaults: &[],
                emitted_events: &[],
                version_column: None,
                soft_delete_column: None,
                retention_days: None,
            },
        )
    }

    #[test]
    fn create_table_marks_primary_key_inline() {
        let sql = create_table_sql(&descriptor());
        assert!(sql.contains("id BLOB PRIMARY KEY"), "got: {sql}");
        assert!(sql.contains("title BLOB"), "got: {sql}");
    }

    #[test]
    fn soft_delete_column_is_added_when_not_in_columns() {
        let mut d = descriptor();
        d.lifecycle.soft_delete_column = Some("deleted_at");
        let sql = create_table_sql(&d);
        assert!(sql.contains("deleted_at BLOB"), "got: {sql}");
    }
}
