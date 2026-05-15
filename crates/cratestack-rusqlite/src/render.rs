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

use std::fmt::Write;

use cratestack_sql::{
    Dialect, FilterExpr, FilterOp, FilterValue, ModelDescriptor, OrderClause, OrderTarget,
    RelationFilter, RelationQuantifier, SortDirection, SqlColumnValue, SqlValue,
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
    let mut bind_index = 1usize;
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
        let mut needs_join = false;
        for filter in filters {
            if needs_join {
                where_sql.push_str(" AND ");
            }
            render_filter_expr(
                dialect,
                filter,
                &mut where_sql,
                &mut binds,
                &mut bind_index,
            );
            needs_join = true;
        }
    }

    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }

    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            render_order_clause(clause, &mut sql);
        }
    }

    if let Some(limit_value) = limit {
        sql.push_str(" LIMIT ");
        dialect.write_placeholder(&mut sql, bind_index);
        bind_index += 1;
        binds.push(SqlValue::Int(limit_value));
    }
    if let Some(offset_value) = offset {
        sql.push_str(" OFFSET ");
        dialect.write_placeholder(&mut sql, bind_index);
        binds.push(SqlValue::Int(offset_value));
    }

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
    let mut binds = vec![id];
    dialect.write_placeholder(&mut sql, 1);
    if let Some(deleted_at) = descriptor.soft_delete_column {
        let _ = write!(&mut sql, " AND {deleted_at} IS NULL");
    }
    (sql, binds.drain(..).collect())
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

/// Render a bulk UPDATE-by-predicate. `set` is the patch column list; the
/// `filters` are AND-joined into the WHERE clause and bind positionally
/// after the SET values. Soft-delete column (if any) is layered in as an
/// implicit `WHERE col IS NULL`. `@version` is auto-incremented for every
/// matched row — bulk update isn't an optimistic-locking idiom, so callers
/// who need CAS should fall back to the per-row `update().if_match()`.
pub fn render_update_many<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    set: &[SqlColumnValue],
    filters: &[FilterExpr],
) -> (String, Vec<SqlValue>) {
    let mut sql = format!("UPDATE {} SET ", descriptor.table_name);
    let mut binds: Vec<SqlValue> = Vec::with_capacity(set.len() + filters.len());
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
    if let Some(version_col) = descriptor.version_column {
        let _ = write!(&mut sql, ", {version_col} = {version_col} + 1");
    }

    sql.push_str(" WHERE ");
    let mut where_started = false;
    if let Some(col) = descriptor.soft_delete_column {
        let _ = write!(&mut sql, "{col} IS NULL");
        where_started = true;
    }
    if !filters.is_empty() {
        if where_started {
            sql.push_str(" AND ");
        }
        sql.push('(');
        let mut joined = false;
        for filter in filters {
            if joined {
                sql.push_str(" AND ");
            }
            render_filter_expr(dialect, filter, &mut sql, &mut binds, &mut bind_index);
            joined = true;
        }
        sql.push(')');
    }
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, binds)
}

/// Render an `INSERT ... ON CONFLICT (<pk>) DO UPDATE SET ... RETURNING ...`
/// upsert. Mirrors the cratestack-sqlx server path but without the
/// always-transactional `SELECT FOR UPDATE` probe: the rusqlite layer has no
/// audit, no event outbox, and no policies, so there's no consumer that
/// needs to distinguish the insert branch from the update branch at the
/// runtime level.
///
/// The DO UPDATE clause uses only columns in `descriptor.upsert_update_columns`
/// (PK, `@version`, `@readonly`, `@server_only`, `@default(...)` excluded by
/// the macro). The `@version` column, when present, is incremented via
/// `<table>.<col> + 1` so concurrent upserts converge to the right value.
pub fn render_upsert<M, PK>(
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
    let _ = write!(
        &mut sql,
        ") ON CONFLICT ({}) DO UPDATE SET ",
        descriptor.primary_key,
    );
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

pub(crate) fn render_filter_expr(
    dialect: &dyn Dialect,
    filter: &FilterExpr,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    match filter {
        FilterExpr::Filter(filter) => match filter.op {
            FilterOp::Eq => render_binary(dialect, &filter.column, "=", &filter.value, sql, binds, bind_index),
            FilterOp::Ne => render_binary(dialect, &filter.column, "!=", &filter.value, sql, binds, bind_index),
            FilterOp::Lt => render_binary(dialect, &filter.column, "<", &filter.value, sql, binds, bind_index),
            FilterOp::Lte => render_binary(dialect, &filter.column, "<=", &filter.value, sql, binds, bind_index),
            FilterOp::Gt => render_binary(dialect, &filter.column, ">", &filter.value, sql, binds, bind_index),
            FilterOp::Gte => render_binary(dialect, &filter.column, ">=", &filter.value, sql, binds, bind_index),
            FilterOp::In => {
                let FilterValue::Many(values) = &filter.value else {
                    unreachable!("FilterOp::In requires FilterValue::Many");
                };
                sql.push_str(filter.column);
                sql.push_str(" IN (");
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        sql.push_str(", ");
                    }
                    dialect.write_placeholder(sql, *bind_index);
                    *bind_index += 1;
                    binds.push(value.clone());
                }
                sql.push(')');
            }
            FilterOp::Contains | FilterOp::StartsWith => {
                render_binary(dialect, &filter.column, "LIKE", &filter.value, sql, binds, bind_index)
            }
            FilterOp::IsNull => {
                let _ = write!(sql, "{} IS NULL", filter.column);
            }
            FilterOp::IsNotNull => {
                let _ = write!(sql, "{} IS NOT NULL", filter.column);
            }
        },
        FilterExpr::All(filters) => render_group(dialect, filters, " AND ", sql, binds, bind_index),
        FilterExpr::Any(filters) => render_group(dialect, filters, " OR ", sql, binds, bind_index),
        FilterExpr::Not(filter) => {
            sql.push_str("NOT (");
            render_filter_expr(dialect, filter, sql, binds, bind_index);
            sql.push(')');
        }
        FilterExpr::Relation(relation) => {
            render_relation(dialect, relation, sql, binds, bind_index);
        }
    }
}

fn render_relation(
    dialect: &dyn Dialect,
    relation: &RelationFilter,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            let _ = write!(
                sql,
                "EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push(')');
        }
        RelationQuantifier::None => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push(')');
        }
        RelationQuantifier::Every => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND NOT (",
                relation.related_table,
                relation.related_table,
                relation.related_column,
                relation.parent_table,
                relation.parent_column,
            );
            render_filter_expr(dialect, &relation.filter, sql, binds, bind_index);
            sql.push_str("))");
        }
    }
}

fn render_binary(
    dialect: &dyn Dialect,
    column: &str,
    op: &str,
    value: &FilterValue,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    let FilterValue::Single(value) = value else {
        unreachable!("binary filter ops require FilterValue::Single");
    };
    let _ = write!(sql, "{column} {op} ");
    dialect.write_placeholder(sql, *bind_index);
    *bind_index += 1;
    binds.push(value.clone());
}

fn render_group(
    dialect: &dyn Dialect,
    filters: &[FilterExpr],
    joiner: &str,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    sql.push('(');
    for (idx, filter) in filters.iter().enumerate() {
        if idx > 0 {
            sql.push_str(joiner);
        }
        render_filter_expr(dialect, filter, sql, binds, bind_index);
    }
    sql.push(')');
}

fn render_order_clause(clause: &OrderClause, sql: &mut String) {
    match &clause.target {
        OrderTarget::Column(column) => {
            let _ = write!(sql, "{column} {} NULLS LAST", sort_dir(clause.direction));
        }
        OrderTarget::RelationScalar {
            parent_table,
            parent_column,
            related_table,
            related_column,
            value_sql,
        } => {
            let _ = write!(
                sql,
                "(SELECT {value_sql} FROM {related_table} WHERE {related_table}.{related_column} = {parent_table}.{parent_column} LIMIT 1) {} NULLS LAST",
                sort_dir(clause.direction),
            );
        }
    }
}

fn sort_dir(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
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
            &[],
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
    fn update_many_with_filter_renders_set_and_where() {
        let dialect = SqliteDialect;
        let descriptor = fixture_descriptor();
        let title_filter = FieldRef::<(), String>::new("title").eq("foo");
        let (sql, binds) = render_update_many(
            &dialect,
            &descriptor,
            &[SqlColumnValue {
                column: "published",
                value: SqlValue::Bool(true),
            }],
            &[FilterExpr::from(title_filter)],
        );
        assert_eq!(
            sql,
            "UPDATE posts SET published = ?1 WHERE (title = ?2) RETURNING id AS \"id\", title AS \"title\", published AS \"published\"",
        );
        assert_eq!(
            binds,
            vec![SqlValue::Bool(true), SqlValue::String("foo".into())],
        );
    }

    #[test]
    fn update_many_with_soft_delete_layers_in_isnull_clause() {
        let dialect = SqliteDialect;
        let descriptor = soft_delete_descriptor();
        let (sql, _) = render_update_many(
            &dialect,
            &descriptor,
            &[SqlColumnValue {
                column: "title",
                value: SqlValue::String("renamed".into()),
            }],
            &[FilterExpr::from(FieldRef::<(), bool>::new("published").is_true())],
        );
        assert!(sql.contains("WHERE deleted_at IS NULL AND ("), "got: {sql}");
    }

    #[test]
    fn order_by_appends_nulls_last() {
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
        assert!(sql.contains("ORDER BY title ASC NULLS LAST"), "got: {sql}");
    }
}
