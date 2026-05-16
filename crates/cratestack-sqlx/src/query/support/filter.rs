//! Top-level filter expression pusher — dispatches each `FilterExpr`
//! variant to its renderer (subkinds defer to [`super::filter_subkinds`]).

use cratestack_sql::{FilterOp, FilterValue};

use crate::{FilterExpr, RelationFilter, RelationQuantifier, sqlx};

use super::filter_subkinds::{
    push_coalesce_filter_query, push_json_filter_query, push_spatial_filter_query,
};
use super::values::push_bind_value;

pub(crate) fn push_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &[FilterExpr],
) {
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(" AND ");
        }
        push_filter_expr_query(query, filter);
    }
}

pub(crate) fn push_filter_expr_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &FilterExpr,
) {
    match filter {
        FilterExpr::Filter(filter) => match filter.op {
            FilterOp::Eq => push_binary_filter_query(query, filter.column, "=", &filter.value),
            FilterOp::Ne => push_binary_filter_query(query, filter.column, "!=", &filter.value),
            FilterOp::Lt => push_binary_filter_query(query, filter.column, "<", &filter.value),
            FilterOp::Lte => push_binary_filter_query(query, filter.column, "<=", &filter.value),
            FilterOp::Gt => push_binary_filter_query(query, filter.column, ">", &filter.value),
            FilterOp::Gte => push_binary_filter_query(query, filter.column, ">=", &filter.value),
            FilterOp::In => {
                query.push(filter.column).push(" IN (");
                let FilterValue::Many(values) = &filter.value else {
                    unreachable!();
                };
                for (value_index, value) in values.iter().enumerate() {
                    if value_index > 0 {
                        query.push(", ");
                    }
                    push_bind_value(query, value);
                }
                query.push(")");
            }
            FilterOp::Contains | FilterOp::StartsWith => {
                push_binary_filter_query(query, filter.column, "LIKE", &filter.value)
            }
            FilterOp::IsNull => {
                query.push(filter.column).push(" IS NULL");
            }
            FilterOp::IsNotNull => {
                query.push(filter.column).push(" IS NOT NULL");
            }
            FilterOp::EqOrNull => {
                let FilterValue::Single(value) = &filter.value else {
                    unreachable!("FilterOp::EqOrNull requires FilterValue::Single");
                };
                query.push("(").push(filter.column).push(" IS NULL OR ");
                query.push(filter.column).push(" = ");
                push_bind_value(query, value);
                query.push(")");
            }
        },
        FilterExpr::All(filters) => push_grouped_filter_query(query, filters, " AND "),
        FilterExpr::Any(filters) => push_grouped_filter_query(query, filters, " OR "),
        FilterExpr::Not(filter) => {
            query.push("NOT (");
            push_filter_expr_query(query, filter);
            query.push(")");
        }
        FilterExpr::Relation(relation) => push_relation_filter_query(query, relation),
        FilterExpr::Coalesce(coalesce) => push_coalesce_filter_query(query, coalesce),
        FilterExpr::Json(filter) => push_json_filter_query(query, filter),
        FilterExpr::Spatial(filter) => push_spatial_filter_query(query, filter),
    }
}

fn push_relation_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    relation: &RelationFilter,
) {
    match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            query.push("EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND ");
            push_filter_expr_query(query, &relation.filter);
            query.push(")");
        }
        RelationQuantifier::None => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND ");
            push_filter_expr_query(query, &relation.filter);
            query.push(")");
        }
        RelationQuantifier::Every => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND NOT (");
            push_filter_expr_query(query, &relation.filter);
            query.push("))");
        }
    }
}

fn push_binary_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    column: &str,
    operator: &str,
    value: &FilterValue,
) {
    query.push(column).push(" ").push(operator).push(" ");
    let FilterValue::Single(value) = value else {
        unreachable!();
    };
    push_bind_value(query, value);
}

fn push_grouped_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &[FilterExpr],
    joiner: &str,
) {
    query.push("(");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(joiner);
        }
        push_filter_expr_query(query, filter);
    }
    query.push(")");
}
