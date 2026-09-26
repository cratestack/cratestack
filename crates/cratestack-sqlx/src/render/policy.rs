//! Read-policy SQL rendering. Allow/deny policies become
//! `(allow_or_clause) AND NOT (deny_or_clause)`; each `PolicyExpr`
//! renders to a `TRUE`/`FALSE` constant or a parameterized predicate
//! based on the per-`ctx` evaluation.

use cratestack_core::CratestackContext;

use crate::{PolicyExpr, ReadPolicy, RelationQuantifier};

use super::policy_predicate::render_policy_predicate;
use super::relation::relation_from_sql;

/// Render a read policy (allow/deny clauses) as a single, fully
/// parenthesized boolean expression.
///
/// The outer parentheses are load-bearing, not cosmetic. This function
/// guarantees a self-contained boolean group, so callers can safely
/// splice the output after `AND` without reintroducing precedence bugs
/// (see `push_action_policy_query` for the asymmetric alternate form).
/// Regression test: `tests_read_policy_predicates::deny_beats_allow_precedence`.
pub(crate) fn render_read_policy_sql(
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CratestackContext,
    bind_index: &mut usize,
) -> Option<String> {
    // Same text, and the same bind order, as `push_action_policy_query`:
    // deny first (it is emitted first), and an empty allow list is `FALSE`
    // even when deny rules exist — otherwise a preview's `$n` numbering
    // drifts from the executed query's whenever both lists bind values.
    let render_or_false = |policies: &[ReadPolicy], bind_index: &mut usize| {
        if policies.is_empty() {
            Some("FALSE".to_owned())
        } else {
            render_allow_policy_sql(policies, ctx, bind_index)
        }
    };
    if deny_policies.is_empty() {
        let allow_sql = render_or_false(allow_policies, bind_index)?;
        return Some(format!("({allow_sql})"));
    }

    let deny_sql = render_allow_policy_sql(deny_policies, ctx, bind_index)?;
    let allow_sql = render_or_false(allow_policies, bind_index)?;
    Some(format!("(NOT ({deny_sql}) AND ({allow_sql}))"))
}

fn render_allow_policy_sql(
    policies: &[ReadPolicy],
    ctx: &CratestackContext,
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
    ctx: &CratestackContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match expr {
        PolicyExpr::Predicate(predicate) => {
            render_policy_predicate(predicate, ctx, sql, bind_index)
        }
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
    ctx: &CratestackContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    // Same correlation as the executed pusher, self-relations included —
    // see `super::relation::relation_from_sql`.
    let (open, negate) = match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 ", false),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 ", false),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 ", true),
    };
    sql.push_str(open);
    sql.push_str(&relation_from_sql(
        parent_table,
        parent_column,
        related_table,
        related_column,
    ));
    sql.push_str(if negate { " AND NOT (" } else { " AND " });
    render_policy_expr_sql(*expr, ctx, sql, bind_index);
    sql.push_str(if negate { "))" } else { ")" });
}

fn render_grouped_policy_sql(
    exprs: &[PolicyExpr],
    joiner: &str,
    ctx: &CratestackContext,
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
