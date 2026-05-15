use std::fmt::Write;

use cratestack_core::CoolContext;
use cratestack_policy::{context_has_role, context_in_tenant};

use cratestack_sql::{FilterOp, FilterValue, OrderTarget};

use crate::{
    FilterExpr, ModelDescriptor, OrderClause, PolicyExpr, ReadPolicy, ReadPredicate,
    RelationFilter, RelationQuantifier, SortDirection,
    query::auth_value_to_sql, query::value_matches_auth_literal,
};

pub(crate) fn render_scoped_select_sql<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
    ctx: &CoolContext,
) -> String {
    let mut sql = format!(
        "SELECT {} FROM {}",
        descriptor.select_projection(),
        descriptor.table_name,
    );
    let mut bind_index = 1usize;
    let user_clause = render_filter_sql(filters, &mut bind_index);
    let policy_clause = render_read_policy_sql(
        descriptor.read_allow_policies,
        descriptor.read_deny_policies,
        ctx,
        &mut bind_index,
    );

    match (user_clause, policy_clause) {
        (Some(user_clause), Some(policy_clause)) => {
            let _ = write!(sql, " WHERE {user_clause} AND ({policy_clause})");
        }
        (Some(user_clause), None) => {
            let _ = write!(sql, " WHERE {user_clause}");
        }
        (None, Some(policy_clause)) => {
            let _ = write!(sql, " WHERE {policy_clause}");
        }
        (None, None) => {}
    }

    if !order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            render_order_clause_sql(clause, &mut sql);
        }
    }

    match (limit, offset) {
        (Some(_), Some(_)) => {
            let _ = write!(sql, " LIMIT ${bind_index} OFFSET ${}", bind_index + 1);
        }
        (Some(_), None) => {
            let _ = write!(sql, " LIMIT ${bind_index}");
        }
        (None, Some(_)) => {
            let _ = write!(sql, " OFFSET ${bind_index}");
        }
        (None, None) => {}
    }

    sql
}

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

fn render_spatial_filter_sql(
    filter: &cratestack_sql::SpatialFilter,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match filter {
        cratestack_sql::SpatialFilter::CoversGeographyPoint { column, .. } => {
            let _ = write!(
                sql,
                "ST_Covers({column}::geography, ST_MakePoint(${lng}, ${lat})::geography)",
                lng = *bind_index,
                lat = *bind_index + 1,
            );
            *bind_index += 2;
        }
        cratestack_sql::SpatialFilter::DWithinGeographyPoint { column, .. } => {
            let _ = write!(
                sql,
                "ST_DWithin({column}::geography, ST_MakePoint(${lng}, ${lat})::geography, ${rad})",
                lng = *bind_index,
                lat = *bind_index + 1,
                rad = *bind_index + 2,
            );
            *bind_index += 3;
        }
    }
}

fn render_json_filter_sql(
    filter: &cratestack_sql::JsonFilter,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match filter {
        cratestack_sql::JsonFilter::HasKey { column, key: _ } => {
            let _ = write!(sql, "{column} ? ${bind_index}");
            *bind_index += 1;
        }
        cratestack_sql::JsonFilter::GetText {
            column,
            key: _,
            op,
            value: _,
        } => {
            let _ = write!(sql, "{column} ->> ${bind_index}");
            *bind_index += 1;
            match op {
                FilterOp::Eq => render_json_text_binary_sql("=", sql, bind_index),
                FilterOp::Ne => render_json_text_binary_sql("!=", sql, bind_index),
                FilterOp::Lt => render_json_text_binary_sql("<", sql, bind_index),
                FilterOp::Lte => render_json_text_binary_sql("<=", sql, bind_index),
                FilterOp::Gt => render_json_text_binary_sql(">", sql, bind_index),
                FilterOp::Gte => render_json_text_binary_sql(">=", sql, bind_index),
                FilterOp::IsNull => sql.push_str(" IS NULL"),
                FilterOp::IsNotNull => sql.push_str(" IS NOT NULL"),
                FilterOp::In
                | FilterOp::Contains
                | FilterOp::StartsWith
                | FilterOp::EqOrNull => {
                    unreachable!(
                        "JsonFilter::GetText built with unsupported op {:?}",
                        op,
                    );
                }
            }
        }
    }
}

fn render_json_text_binary_sql(operator: &str, sql: &mut String, bind_index: &mut usize) {
    let _ = write!(sql, " {operator} ${bind_index}");
    *bind_index += 1;
}

fn render_coalesce_filter_sql(
    filter: &cratestack_sql::CoalesceFilter,
    sql: &mut String,
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
        FilterOp::Eq => render_coalesce_binary_sql("=", sql, bind_index),
        FilterOp::Ne => render_coalesce_binary_sql("!=", sql, bind_index),
        FilterOp::Lt => render_coalesce_binary_sql("<", sql, bind_index),
        FilterOp::Lte => render_coalesce_binary_sql("<=", sql, bind_index),
        FilterOp::Gt => render_coalesce_binary_sql(">", sql, bind_index),
        FilterOp::Gte => render_coalesce_binary_sql(">=", sql, bind_index),
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

fn render_coalesce_binary_sql(operator: &str, sql: &mut String, bind_index: &mut usize) {
    let _ = write!(sql, " {operator} ${bind_index}");
    *bind_index += 1;
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

pub(crate) fn render_read_policy_sql(
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
    bind_index: &mut usize,
) -> Option<String> {
    if allow_policies.is_empty() {
        return Some("FALSE".to_owned());
    }

    let allow_sql = render_allow_policy_sql(allow_policies, ctx, bind_index)?;
    if deny_policies.is_empty() {
        return Some(allow_sql);
    }

    let deny_sql = render_allow_policy_sql(deny_policies, ctx, bind_index)?;
    Some(format!("NOT ({deny_sql}) AND ({allow_sql})"))
}

fn render_allow_policy_sql(
    policies: &[ReadPolicy],
    ctx: &CoolContext,
    bind_index: &mut usize,
) -> Option<String> {
    if policies.is_empty() {
        return None;
    }

    let mut sql = String::new();
    for (policy_index, policy) in policies.iter().enumerate() {
        if policy_index > 0 {
            sql.push_str(" OR ");
        }
        render_policy_expr_sql(policy.expr, ctx, &mut sql, bind_index);
    }

    Some(sql)
}

pub(crate) fn render_policy_expr_sql(
    expr: PolicyExpr,
    ctx: &CoolContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match expr {
        PolicyExpr::Predicate(predicate) => match predicate {
            ReadPredicate::AuthNotNull => {
                sql.push_str(if ctx.is_authenticated() {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::AuthIsNull => {
                sql.push_str(if ctx.is_authenticated() {
                    "FALSE"
                } else {
                    "TRUE"
                });
            }
            ReadPredicate::HasRole { role } => {
                sql.push_str(if context_has_role(ctx, role) {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::InTenant { tenant_id } => {
                sql.push_str(if context_in_tenant(ctx, tenant_id) {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::AuthFieldEqLiteral { auth_field, value } => {
                sql.push_str(
                    if ctx
                        .auth_field(auth_field)
                        .is_some_and(|candidate| value_matches_auth_literal(candidate, value))
                    {
                        "TRUE"
                    } else {
                        "FALSE"
                    },
                );
            }
            ReadPredicate::AuthFieldNeLiteral { auth_field, value } => {
                sql.push_str(
                    if ctx
                        .auth_field(auth_field)
                        .is_some_and(|candidate| !value_matches_auth_literal(candidate, value))
                    {
                        "TRUE"
                    } else {
                        "FALSE"
                    },
                );
            }
            ReadPredicate::FieldIsTrue { column } => {
                let _ = write!(sql, "{column} = TRUE");
            }
            ReadPredicate::FieldEqLiteral { column, .. } => {
                let _ = write!(sql, "{column} = ${bind_index}");
                *bind_index += 1;
            }
            ReadPredicate::FieldNeLiteral { column, .. } => {
                let _ = write!(sql, "{column} != ${bind_index}");
                *bind_index += 1;
            }
            ReadPredicate::FieldEqAuth { column, auth_field } => {
                if auth_value_to_sql(ctx, auth_field).is_some() {
                    let _ = write!(sql, "{column} = ${bind_index}");
                    *bind_index += 1;
                } else {
                    sql.push_str("FALSE");
                }
            }
            ReadPredicate::FieldNeAuth { column, auth_field } => {
                if auth_value_to_sql(ctx, auth_field).is_some() {
                    let _ = write!(sql, "{column} != ${bind_index}");
                    *bind_index += 1;
                } else {
                    sql.push_str("FALSE");
                }
            }
            ReadPredicate::Relation {
                quantifier,
                parent_table,
                parent_column,
                related_table,
                related_column,
                expr,
            } => {
                render_relation_policy_sql(
                    quantifier,
                    parent_table,
                    parent_column,
                    related_table,
                    related_column,
                    expr,
                    ctx,
                    sql,
                    bind_index,
                );
            }
        },
        PolicyExpr::And(exprs) => render_grouped_policy_sql(exprs, " AND ", ctx, sql, bind_index),
        PolicyExpr::Or(exprs) => render_grouped_policy_sql(exprs, " OR ", ctx, sql, bind_index),
    }
}

fn render_relation_policy_sql(
    quantifier: RelationQuantifier,
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
    expr: &'static PolicyExpr,
    ctx: &CoolContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            render_relation_policy_exists_sql(
                sql,
                bind_index,
                parent_table,
                parent_column,
                related_table,
                related_column,
                &|sql, bind_index| render_policy_expr_sql(*expr, ctx, sql, bind_index),
            );
        }
        RelationQuantifier::None => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                related_table, related_table, related_column, parent_table, parent_column,
            );
            render_policy_expr_sql(*expr, ctx, sql, bind_index);
            sql.push(')');
        }
        RelationQuantifier::Every => {
            let _ = write!(
                sql,
                "NOT EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND NOT (",
                related_table, related_table, related_column, parent_table, parent_column,
            );
            render_policy_expr_sql(*expr, ctx, sql, bind_index);
            sql.push_str("))");
        }
    }
}

fn render_relation_policy_exists_sql<Render>(
    sql: &mut String,
    bind_index: &mut usize,
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
    render_predicate: &Render,
) where
    Render: Fn(&mut String, &mut usize),
{
    let _ = write!(
        sql,
        "EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
        related_table, related_table, related_column, parent_table, parent_column,
    );
    render_predicate(sql, bind_index);
    sql.push(')');
}

pub(crate) fn render_order_clause_sql(clause: &OrderClause, sql: &mut String) {
    match &clause.target {
        OrderTarget::Column(column) => {
            let _ = write!(
                sql,
                "{} {} {}",
                column,
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
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
                "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1) {} {}",
                value_sql,
                related_table,
                related_table,
                related_column,
                parent_table,
                parent_column,
                sort_direction_sql(clause.direction),
                null_order_sql(clause.null_order),
            );
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

fn render_grouped_policy_sql(
    exprs: &[PolicyExpr],
    joiner: &str,
    ctx: &CoolContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    sql.push('(');
    for (index, expr) in exprs.iter().enumerate() {
        if index > 0 {
            sql.push_str(joiner);
        }
        render_policy_expr_sql(*expr, ctx, sql, bind_index);
    }
    sql.push(')');
}

fn sort_direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

fn null_order_sql(order: cratestack_sql::NullOrder) -> &'static str {
    match order {
        cratestack_sql::NullOrder::First => "NULLS FIRST",
        cratestack_sql::NullOrder::Last => "NULLS LAST",
    }
}
