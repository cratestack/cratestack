//! Read-policy SQL rendering. Allow/deny policies become
//! `(allow_or_clause) AND NOT (deny_or_clause)`; each `PolicyExpr`
//! renders to a `TRUE`/`FALSE` constant or a parameterized predicate
//! based on the per-`ctx` evaluation.

use std::fmt::Write;

use cratestack_core::CoolContext;

use crate::{PolicyExpr, ReadPolicy, RelationQuantifier};

use super::policy_predicate::render_policy_predicate;

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
        PolicyExpr::Predicate(predicate) => render_policy_predicate(predicate, ctx, sql, bind_index),
        PolicyExpr::And(exprs) => render_grouped_policy_sql(exprs, " AND ", ctx, sql, bind_index),
        PolicyExpr::Or(exprs) => render_grouped_policy_sql(exprs, " OR ", ctx, sql, bind_index),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_relation_policy_sql(
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
            let _ = write!(
                sql,
                "EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{} AND ",
                related_table, related_table, related_column, parent_table, parent_column,
            );
            render_policy_expr_sql(*expr, ctx, sql, bind_index);
            sql.push(')');
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
