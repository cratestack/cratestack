//! Allow/deny policy pushers — top-level dispatch + the per-action
//! combinator (deny rules sit inside a `NOT (...)`, allow rules
//! disjoin). Predicate emission lives in
//! [`super::policy_predicate`]; relation policies in
//! [`super::policy_relation`].

use cratestack_core::CoolContext;

use crate::{PolicyExpr, ReadPolicy, sqlx};

use super::policy_predicate::push_policy_predicate;

/// Emits the action's policy predicate as a **single, fully
/// parenthesized** boolean expression.
///
/// The outer parentheses are load-bearing, not cosmetic. Every call site
/// splices this directly after `<row filter> AND ` (see
/// `authorize_record_action` / `push_scoped_conditions` in
/// `query/support/conditions.rs`, and every `query/write/*_exec.rs` and
/// `query/batch/*.rs` mutation path). A model may declare several
/// `@@allow("<action>", ...)` clauses, which disjoin into `A OR B`; since
/// SQL's `AND` binds tighter than `OR`, emitting that bare would make
/// `id = $1 AND A OR B` parse as `(id = $1 AND A) OR B` — the row filter
/// would scope only the first clause, and any row in the table matching
/// `B` alone would satisfy the predicate. That is an authorization
/// bypass on reads and, on the write paths, operates on rows other than
/// the targeted one. Wrapping here keeps callers from having to know
/// this. Regression: `crate::tests_policy_precedence_bug`.
pub(crate) fn push_action_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    query.push("(");
    if !deny_policies.is_empty() {
        query.push("NOT (");
        push_allow_policy_query(query, deny_policies, ctx);
        query.push(") AND (");
        push_allow_policy_query(query, allow_policies, ctx);
        query.push(")");
    } else {
        push_allow_policy_query(query, allow_policies, ctx);
    }
    query.push(")");
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
