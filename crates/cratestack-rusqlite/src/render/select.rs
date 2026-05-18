//! `SELECT ... FROM table WHERE ... ORDER BY ... LIMIT/OFFSET` and
//! single-PK select rendering.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterExpr, OrderClause, ReadSource, SqlValue};

use super::filter::render_filter_expr;
use super::order::render_order_clause;

/// Render a `SELECT ... FROM table WHERE ... ORDER BY ... LIMIT ?N OFFSET ?N`
/// statement and return it alongside the values that bind into the
/// placeholders, in placeholder order.
pub fn render_select<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &dyn ReadSource<M, PK>,
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection(),
        descriptor.table_name(),
    );
    let mut binds: Vec<SqlValue> = Vec::new();
    let mut bind_index = 1usize;
    let mut where_sql = String::new();
    let mut soft_delete_active = false;

    if let Some(deleted_at) = descriptor.soft_delete_column() {
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
            render_filter_expr(dialect, filter, &mut where_sql, &mut binds, &mut bind_index);
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
    descriptor: &dyn ReadSource<M, PK>,
    id: SqlValue,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!(
        "SELECT {} FROM {} WHERE {} = ",
        descriptor.select_projection(),
        descriptor.table_name(),
        descriptor.primary_key(),
    );
    let mut binds = vec![id];
    dialect.write_placeholder(&mut sql, 1);
    if let Some(deleted_at) = descriptor.soft_delete_column() {
        let _ = write!(&mut sql, " AND {deleted_at} IS NULL");
    }
    (sql, binds.drain(..).collect())
}
