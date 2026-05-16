//! Top-level filter dispatch: `FilterExpr` → SQL fragment + bind list.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterExpr, FilterOp, FilterValue, SqlValue};

use super::{
    coalesce::render_coalesce, json::render_json, relation::render_relation,
};

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
            FilterOp::EqOrNull => {
                let FilterValue::Single(value) = &filter.value else {
                    unreachable!("FilterOp::EqOrNull requires FilterValue::Single");
                };
                let _ = write!(sql, "({col} IS NULL OR {col} = ", col = filter.column);
                dialect.write_placeholder(sql, *bind_index);
                *bind_index += 1;
                binds.push(value.clone());
                sql.push(')');
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
        FilterExpr::Coalesce(coalesce) => {
            render_coalesce(dialect, coalesce, sql, binds, bind_index);
        }
        FilterExpr::Json(json) => {
            render_json(dialect, json, sql, binds, bind_index);
        }
        FilterExpr::Spatial(_) => {
            // PostGIS-style spatial predicates require server-side
            // extensions (PostGIS on Postgres, SpatiaLite on SQLite)
            // that the embedded runtime doesn't ship by default. We
            // fail loud at render time rather than silently emitting
            // SQL the SQLite parser would reject anyway — a schema
            // that uses spatial filters is implicitly server-only.
            panic!(
                "spatial filters are not supported on the embedded rusqlite backend; \
                 schemas that use FieldRef::covers_geography / dwithin_geography are server-only",
            );
        }
    }
}

pub(super) fn render_binary(
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
