//! Bulk DELETE-by-predicate rendering. Soft-delete-aware (UPDATE-of-
//! `deleted_at` + bump `@version` if any). Filters are required at the
//! builder level — empty WHERE table-wipes are rejected upstream.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterExpr, ModelDescriptor, SqlValue};

use super::filter::render_filter_expr;

pub fn render_delete_many<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
) -> (String, Vec<SqlValue>) {
    let mut sql = String::new();
    let mut binds: Vec<SqlValue> = Vec::with_capacity(filters.len());
    let mut bind_index = 1usize;

    let mut where_started = false;
    match descriptor.soft_delete_column {
        Some(col) => {
            let _ = write!(
                &mut sql,
                "UPDATE {} SET {col} = CURRENT_TIMESTAMP",
                descriptor.table_name,
            );
            if let Some(version_col) = descriptor.version_column {
                let _ = write!(&mut sql, ", {version_col} = {version_col} + 1");
            }
            sql.push_str(" WHERE ");
            let _ = write!(&mut sql, "{col} IS NULL");
            where_started = true;
        }
        None => {
            let _ = write!(&mut sql, "DELETE FROM {} WHERE ", descriptor.table_name);
        }
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
