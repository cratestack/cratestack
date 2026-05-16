//! Allow/deny policy pushers — top-level dispatch + the per-action
//! combinator (deny rules sit inside a `NOT (...)`, allow rules
//! disjoin). Predicate emission lives in
//! [`super::policy_predicate`]; relation policies in
//! [`super::policy_relation`].

use cratestack_core::CoolContext;

use crate::{PolicyExpr, ReadPolicy, sqlx};

use super::policy_predicate::push_policy_predicate;

pub(crate) fn push_action_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    if !deny_policies.is_empty() {
        query.push("NOT (");
        push_allow_policy_query(query, deny_policies, ctx);
        query.push(") AND (");
        push_allow_policy_query(query, allow_policies, ctx);
        query.push(")");
    } else {
        push_allow_policy_query(query, allow_policies, ctx);
    }
}

fn push_allow_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    if policies.is_empty() {
        query.push("FALSE");
        return;
    }

    for (policy_index, policy) in policies.iter().enumerate() {
        if policy_index > 0 {
            query.push(" OR ");
        }
        push_policy_expr_query(query, policy.expr, ctx);
    }
}

pub(crate) fn push_policy_expr_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    expr: PolicyExpr,
    ctx: &CoolContext,
) {
    match expr {
        PolicyExpr::Predicate(predicate) => push_policy_predicate(query, predicate, ctx),
        PolicyExpr::And(exprs) => push_grouped_policy_query(query, exprs, " AND ", ctx),
        PolicyExpr::Or(exprs) => push_grouped_policy_query(query, exprs, " OR ", ctx),
    }
}

fn push_grouped_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    exprs: &[PolicyExpr],
    joiner: &str,
    ctx: &CoolContext,
) {
    query.push("(");
    for (index, expr) in exprs.iter().enumerate() {
        if index > 0 {
            query.push(joiner);
        }
        push_policy_expr_query(query, *expr, ctx);
    }
    query.push(")");
}
