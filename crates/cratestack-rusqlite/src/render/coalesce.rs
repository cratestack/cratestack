//! `CoalesceFilter` rendering via SQLite's `COALESCE(...)` function.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterOp, FilterValue, SqlValue};

pub(super) fn render_coalesce(
    dialect: &dyn Dialect,
    filter: &cratestack_sql::CoalesceFilter,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    sql.push_str("COALESCE(");
    for (idx, column) in filter.columns.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(column);
    }
    sql.push(')');
    match filter.op {
        FilterOp::Eq => render_coalesce_binary(dialect, "=", &filter.value, sql, binds, bind_index),
        FilterOp::Ne => render_coalesce_binary(dialect, "!=", &filter.value, sql, binds, bind_index),
        FilterOp::Lt => render_coalesce_binary(dialect, "<", &filter.value, sql, binds, bind_index),
        FilterOp::Lte => render_coalesce_binary(dialect, "<=", &filter.value, sql, binds, bind_index),
        FilterOp::Gt => render_coalesce_binary(dialect, ">", &filter.value, sql, binds, bind_index),
        FilterOp::Gte => render_coalesce_binary(dialect, ">=", &filter.value, sql, binds, bind_index),
        FilterOp::IsNull => sql.push_str(" IS NULL"),
        FilterOp::IsNotNull => sql.push_str(" IS NOT NULL"),
        FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
            unreachable!(
                "CoalesceFilter built with unsupported op {:?}",
                filter.op,
            );
        }
    }
}

fn render_coalesce_binary(
    dialect: &dyn Dialect,
    operator: &str,
    value: &FilterValue,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    let FilterValue::Single(value) = value else {
        unreachable!("coalesce comparison requires FilterValue::Single");
    };
    let _ = write!(sql, " {operator} ");
    dialect.write_placeholder(sql, *bind_index);
    *bind_index += 1;
    binds.push(value.clone());
}
