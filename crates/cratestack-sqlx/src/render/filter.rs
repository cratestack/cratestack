//! Top-level filter SQL rendering — dispatches each `FilterExpr`
//! variant to its renderer (relation/coalesce/json/spatial defer to
//! [`super::filter_subkinds`]) and emits per-op SQL for scalar
//! comparisons.

use std::fmt::Write;

use cratestack_sql::{FilterOp, FilterValue};

use cratestack_core::CratestackContext;

use crate::FilterExpr;

use super::filter_subkinds::{
    render_coalesce_filter_sql, render_json_filter_sql, render_vector_distance_filter_sql,
};
use super::relation::render_relation_filter_sql;

/// `ctx`: `Some` renders each relation subquery's related read scope as
/// it executes; `None` (the ctx-free `preview_sql`) renders no scope. See
/// `super::relation`.
pub(crate) fn render_filter_sql(
    filters: &[FilterExpr],
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) -> Option<String> {
    if filters.is_empty() {
        return None;
    }

    let mut sql = String::new();
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(" AND ");
        }
        render_filter_expr_sql(filter, &mut sql, bind_index, ctx);
    }

    Some(sql)
}

pub(crate) fn render_filter_expr_sql(
    filter: &FilterExpr,
    sql: &mut String,
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) {
    match filter {
        FilterExpr::Filter(filter) => match filter.op {
            FilterOp::Eq => render_binary_filter_sql(filter.column, "=", sql, bind_index),
            FilterOp::Ne => render_binary_filter_sql(filter.column, "!=", sql, bind_index),
            FilterOp::Lt => render_binary_filter_sql(filter.column, "<", sql, bind_index),
            FilterOp::Lte => render_binary_filter_sql(filter.column, "<=", sql, bind_index),
            FilterOp::Gt => render_binary_filter_sql(filter.column, ">", sql, bind_index),
            FilterOp::Gte => render_binary_filter_sql(filter.column, ">=", sql, bind_index),
            FilterOp::In => {
                let FilterValue::Many(values) = &filter.value else {
                    unreachable!();
                };
                sql.push_str(filter.column);
                sql.push_str(" IN (");
                for (value_index, _) in values.iter().enumerate() {
                    if value_index > 0 {
                        sql.push_str(", ");
                    }
                    let _ = write!(sql, "${bind_index}");
                    *bind_index += 1;
                }
                sql.push(')');
            }
            FilterOp::Contains | FilterOp::StartsWith => {
                render_binary_filter_sql(filter.column, "LIKE", sql, bind_index)
            }
            FilterOp::IsNull => {
                let _ = write!(sql, "{} IS NULL", filter.column);
            }
            FilterOp::IsNotNull => {
                let _ = write!(sql, "{} IS NOT NULL", filter.column);
            }
            FilterOp::EqOrNull => {
                let _ = write!(
                    sql,
                    "({col} IS NULL OR {col} = ${bind})",
                    col = filter.column,
                    bind = *bind_index,
                );
                *bind_index += 1;
            }
        },
        FilterExpr::All(filters) => {
            render_grouped_filter_sql(filters, " AND ", sql, bind_index, ctx)
        }
        FilterExpr::Any(filters) => {
            render_grouped_filter_sql(filters, " OR ", sql, bind_index, ctx)
        }
        FilterExpr::Not(filter) => {
            sql.push_str("NOT (");
            render_filter_expr_sql(filter, sql, bind_index, ctx);
            sql.push(')');
        }
        FilterExpr::Relation(relation) => {
            render_relation_filter_sql(relation, sql, bind_index, ctx);
        }
        FilterExpr::Coalesce(coalesce) => {
            render_coalesce_filter_sql(coalesce, sql, bind_index);
        }
        FilterExpr::Json(json) => {
            render_json_filter_sql(json, sql, bind_index);
        }
        #[cfg(feature = "postgis")]
        FilterExpr::Spatial(spatial) => {
            super::filter_subkinds::render_spatial_filter_sql(spatial, sql, bind_index);
        }
        FilterExpr::VectorDistance(vector) => {
            render_vector_distance_filter_sql(vector, sql, bind_index);
        }
    }
}

fn render_binary_filter_sql(
    column: &str,
    operator: &str,
    sql: &mut String,
    bind_index: &mut usize,
) {
    let _ = write!(sql, "{column} {operator} ${bind_index}");
    *bind_index += 1;
}

fn render_grouped_filter_sql(
    filters: &[FilterExpr],
    joiner: &str,
    sql: &mut String,
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) {
    sql.push('(');
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(joiner);
        }
        render_filter_expr_sql(filter, sql, bind_index, ctx);
    }
    sql.push(')');
}
