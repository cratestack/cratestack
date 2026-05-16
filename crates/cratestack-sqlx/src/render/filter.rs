//! Top-level filter SQL rendering — dispatches each `FilterExpr`
//! variant to its renderer (relation/coalesce/json/spatial defer to
//! [`super::filter_subkinds`]) and emits per-op SQL for scalar
//! comparisons.

use std::fmt::Write;

use cratestack_sql::{FilterOp, FilterValue};

use crate::{FilterExpr, RelationFilter, RelationQuantifier};

use super::filter_subkinds::{
    render_coalesce_filter_sql, render_json_filter_sql, render_spatial_filter_sql,
};

pub(crate) fn render_filter_sql(filters: &[FilterExpr], bind_index: &mut usize) -> Option<String> {
    if filters.is_empty() {
        return None;
    }

    let mut sql = String::new();
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(" AND ");
        }
        render_filter_expr_sql(filter, &mut sql, bind_index);
    }

    Some(sql)
}

pub(crate) fn render_filter_expr_sql(
    filter: &FilterExpr,
    sql: &mut String,
    bind_index: &mut usize,
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
        FilterExpr::All(filters) => render_grouped_filter_sql(filters, " AND ", sql, bind_index),
        FilterExpr::Any(filters) => render_grouped_filter_sql(filters, " OR ", sql, bind_index),
        FilterExpr::Not(filter) => {
            sql.push_str("NOT (");
            render_filter_expr_sql(filter, sql, bind_index);
            sql.push(')');
        }
        FilterExpr::Relation(relation) => {
            render_relation_filter_sql(relation, sql, bind_index);
        }
        FilterExpr::Coalesce(coalesce) => {
            render_coalesce_filter_sql(coalesce, sql, bind_index);
        }
        FilterExpr::Json(json) => {
            render_json_filter_sql(json, sql, bind_index);
        }
        FilterExpr::Spatial(spatial) => {
            render_spatial_filter_sql(spatial, sql, bind_index);
        }
    }
}

pub(crate) fn render_relation_filter_sql(
    relation: &RelationFilter,
    sql: &mut String,
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
            render_filter_expr_sql(&relation.filter, sql, bind_index);
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
            render_filter_expr_sql(&relation.filter, sql, bind_index);
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
            render_filter_expr_sql(&relation.filter, sql, bind_index);
            sql.push_str("))");
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
) {
    sql.push('(');
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(joiner);
        }
        render_filter_expr_sql(filter, sql, bind_index);
    }
    sql.push(')');
}
