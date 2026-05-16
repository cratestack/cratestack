//! Bulk UPDATE-by-predicate rendering. `@version` auto-bumps every matched
//! row — bulk update isn't an optimistic-locking idiom, so CAS callers fall
//! back to per-row `update().if_match()`.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterExpr, ModelDescriptor, SqlColumnValue, SqlValue};

use super::filter::render_filter_expr;

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
