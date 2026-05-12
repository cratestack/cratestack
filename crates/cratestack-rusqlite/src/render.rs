//! SQLite SQL renderer.
//!
//! No policy support — on device the runtime is single-user and authorization
//! is not enforced at the storage layer. This makes the renderer noticeably
//! simpler than the cratestack-sqlx one: just filters, ordering, paging, and
//! the obvious INSERT/UPDATE/DELETE statements.
//!
//! Output is consumed by `rusqlite::Statement` with positional `?N`
//! placeholders. Bind ordering matches the order in which placeholders are
//! emitted into the SQL string.
//!
//! Filter/order traversal lives in [`cratestack_sql::render`]; this module
//! plugs the SQLite dialect into a `StringSink` and adds the INSERT /
//! UPDATE / DELETE statement skeletons that are still backend-specific.

use std::fmt::Write;

use cratestack_sql::{
    render::{render_filter_exprs, render_order_and_paging},
    Dialect, FilterExpr, ModelDescriptor, OrderClause, SqlColumnValue, SqlValue, StringSink,
};

/// Render a `SELECT ... FROM table WHERE ... ORDER BY ... LIMIT ?N OFFSET ?N`
/// statement and return it alongside the values that bind into the
/// placeholders, in placeholder order.
pub fn render_select<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection(),
        descriptor.table_name,
    );
    let mut binds: Vec<SqlValue> = Vec::new();
    let mut where_sql = String::new();
    let mut soft_delete_active = false;

    if let Some(deleted_at) = descriptor.soft_delete_column {
        let _ = write!(&mut where_sql, "{deleted_at} IS NULL");
        soft_delete_active = true;
    }

    if !filters.is_empty() {
        if soft_delete_active {
            where_sql.push_str(" AND ");
        }
        let mut sink = StringSink::with_binds(&mut where_sql, dialect, 1, &mut binds);
        render_filter_exprs(&mut sink, filters);
    }

    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }

    let next_bind = binds.len() + 1;
    let mut sink = StringSink::with_binds(&mut sql, dialect, next_bind, &mut binds);
    render_order_and_paging(&mut sink, order_by, limit, offset);

    (sql, binds)
}

/// Render `SELECT ... FROM table WHERE pk = ?1 [AND deleted_at IS NULL]`.
pub fn render_select_by_pk<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    id: SqlValue,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!(
        "SELECT {} FROM {} WHERE {} = ",
        descriptor.select_projection(),
        descriptor.table_name,
        descriptor.primary_key,
    );
    dialect.write_placeholder(&mut sql, 1);
    if let Some(deleted_at) = descriptor.soft_delete_column {
        let _ = write!(&mut sql, " AND {deleted_at} IS NULL");
    }
    (sql, vec![id])
}

/// Render an INSERT statement with `RETURNING *`. SQLite supports
/// `RETURNING` since 3.35 (2021); rusqlite's `bundled` feature pulls in a
/// new-enough build.
pub fn render_insert<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    values: &[SqlColumnValue],
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
    sql.push_str(") RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, binds)
}

/// Render an UPDATE statement with `RETURNING *`. The `set` columns are
/// emitted in the order provided; the primary key is bound last.
pub fn render_update<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    set: &[SqlColumnValue],
    id: SqlValue,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!("UPDATE {} SET ", descriptor.table_name);
    let mut binds = Vec::with_capacity(set.len() + 1);
    let mut bind_index = 1usize;
    for (idx, value) in set.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        let _ = write!(&mut sql, "{} = ", value.column);
        dialect.write_placeholder(&mut sql, bind_index);
        bind_index += 1;
        binds.push(value.value.clone());
    }
    let _ = write!(&mut sql, " WHERE {} = ", descriptor.primary_key);
    dialect.write_placeholder(&mut sql, bind_index);
    binds.push(id);
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, binds)
}

/// Render a DELETE statement. For soft-delete-enabled models this becomes
/// an UPDATE-of-`deleted_at` instead, matching the cratestack-sqlx semantics.
pub fn render_delete<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    id: SqlValue,
    now: chrono::DateTime<chrono::Utc>,
) -> (String, Vec<SqlValue>) {
    if let Some(deleted_at) = descriptor.soft_delete_column {
        let mut sql = format!("UPDATE {} SET {deleted_at} = ", descriptor.table_name);
        dialect.write_placeholder(&mut sql, 1);
        let _ = write!(&mut sql, " WHERE {} = ", descriptor.primary_key);
        dialect.write_placeholder(&mut sql, 2);
        sql.push_str(" RETURNING ");
        sql.push_str(&descriptor.select_projection());
        return (sql, vec![SqlValue::DateTime(now), id]);
    }

    let mut sql = format!(
        "DELETE FROM {} WHERE {} = ",
        descriptor.table_name, descriptor.primary_key,
    );
    dialect.write_placeholder(&mut sql, 1);
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, vec![id])
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_sql::{FieldRef, FilterExpr, ModelColumn, SortDirection, SqliteDialect};

    fn fixture_descriptor() -> ModelDescriptor<(), i64> {
        const COLUMNS: &[ModelColumn] = &[
            ModelColumn { rust_name: "id", sql_name: "id" },
            ModelColumn { rust_name: "title", sql_name: "title" },
            ModelColumn { rust_name: "published", sql_name: "published" },
        ];
        ModelDescriptor::new(
            "Post",
            "posts",
            COLUMNS,
            "id",
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            None,
            false,
            &[],
            &[],
            None,
            None,
        )
    }

    fn soft_delete_descriptor() -> ModelDescriptor<(), i64> {
        let mut d = fixture_descriptor();
        d.soft_delete_column = Some("deleted_at");
        d
    }

    #[test]
    fn select_uses_question_placeholders_for_limit_and_offset() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let (sql, binds) = render_select(
            &dialect,
            &descriptor,
            &[],
            &[],
            Some(10),
            Some(5),
        );
        assert!(sql.contains("LIMIT ?1"));
        assert!(sql.contains("OFFSET ?2"));
        assert_eq!(binds, vec![SqlValue::Int(10), SqlValue::Int(5)]);
    }

    #[test]
    fn select_with_filter_emits_where_and_binds_value() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let id_ref = FieldRef::<(), i64>::new("id");
        let (sql, binds) = render_select(
            &dialect,
            &descriptor,
            &[FilterExpr::from(id_ref.eq(42i64))],
            &[],
            None,
            None,
        );
        assert!(sql.contains("WHERE id = ?1"), "got: {sql}");
        assert_eq!(binds, vec![SqlValue::Int(42)]);
    }

    #[test]
    fn select_with_soft_delete_filters_out_deleted_rows() {
        let dialect = SqliteDialect;
        let descriptor = soft_delete_descriptor();
        let (sql, _) = render_select(&dialect, &descriptor, &[], &[], None, None);
        assert!(sql.contains("WHERE deleted_at IS NULL"), "got: {sql}");
    }

    #[test]
    fn select_by_pk_binds_id_and_filters_soft_deletes() {
        let dialect = SqliteDialect;
        let descriptor = soft_delete_descriptor();
        let (sql, binds) = render_select_by_pk(&dialect, &descriptor, SqlValue::Int(7));
        assert!(sql.contains("WHERE id = ?1"));
        assert!(sql.contains("AND deleted_at IS NULL"));
        assert_eq!(binds, vec![SqlValue::Int(7)]);
    }

    #[test]
    fn insert_returns_full_row_projection() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let (sql, binds) = render_insert(
            &dialect,
            &descriptor,
            &[
                SqlColumnValue { column: "title", value: SqlValue::String("hi".into()) },
                SqlColumnValue { column: "published", value: SqlValue::Bool(true) },
            ],
        );
        assert!(sql.starts_with("INSERT INTO posts (title, published) VALUES (?1, ?2)"));
        assert!(sql.contains("RETURNING"));
        assert_eq!(
            binds,
            vec![SqlValue::String("hi".into()), SqlValue::Bool(true)],
        );
    }

    #[test]
    fn update_binds_id_last_and_returns_row() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let (sql, binds) = render_update(
            &dialect,
            &descriptor,
            &[SqlColumnValue { column: "title", value: SqlValue::String("new".into()) }],
            SqlValue::Int(5),
        );
        assert!(sql.starts_with("UPDATE posts SET title = ?1 WHERE id = ?2"));
        assert!(sql.contains("RETURNING"));
        assert_eq!(binds, vec![SqlValue::String("new".into()), SqlValue::Int(5)]);
    }

    #[test]
    fn delete_hard_emits_delete_statement() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let (sql, binds) =
            render_delete(&dialect, &descriptor, SqlValue::Int(9), chrono::Utc::now());
        assert!(sql.starts_with("DELETE FROM posts WHERE id = ?1 RETURNING"));
        assert_eq!(binds, vec![SqlValue::Int(9)]);
    }

    #[test]
    fn delete_soft_emits_update_of_deleted_at() {
        let dialect = SqliteDialect;
        let descriptor = soft_delete_descriptor();
        let now = chrono::Utc::now();
        let (sql, binds) =
            render_delete(&dialect, &descriptor, SqlValue::Int(9), now);
        assert!(sql.starts_with("UPDATE posts SET deleted_at = ?1 WHERE id = ?2"));
        assert_eq!(binds, vec![SqlValue::DateTime(now), SqlValue::Int(9)]);
    }

    #[test]
    fn order_by_renders_plain_column_without_nulls_last() {
        // The pre-refactor SQLite renderer appended `NULLS LAST` to every
        // ORDER BY clause regardless of target shape; sqlx omitted it for
        // plain columns and only added it for relation-scalar subqueries.
        // The shared renderer aligned on the sqlx spelling because tests
        // there pinned the exact form. SQLite's default ASC null ordering
        // ("nulls first") differs from Postgres's, but no SQLite-side
        // contract depended on the old `NULLS LAST` either, so the
        // alignment is observably safe.
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let (sql, _) = render_select(
            &dialect,
            &descriptor,
            &[],
            &[OrderClause::column("title", SortDirection::Asc)],
            None,
            None,
        );
        assert!(sql.contains("ORDER BY title ASC"), "got: {sql}");
        assert!(
            !sql.contains("ORDER BY title ASC NULLS LAST"),
            "plain-column ORDER BY should not append NULLS LAST: {sql}",
        );
    }
}
