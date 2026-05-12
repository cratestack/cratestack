//! Render `cratestack-policy` ASTs into SQL.
//!
//! Sister module to [`crate::render`]: the filter renderer handles user-
//! supplied query AST, this one handles compile-time policy AST baked into
//! `ModelDescriptor`. Both write through the [`SqlSink`] trait so the same
//! traversal serves both `sqlx::QueryBuilder` (run path) and plain `String`
//! (preview path).
//!
//! Pre-evaluation: predicates that depend only on the `CoolContext`
//! (`AuthNotNull`, `HasRole`, `InTenant`, `AuthField*Literal`) are folded to
//! literal `TRUE` / `FALSE` at render time so the resulting SQL never has
//! to consult the context. Predicates that compare a column to an auth
//! value emit a placeholder and bind the auth value.

use cratestack_core::{CoolContext, Value};
use cratestack_policy::{
    context_has_role, context_in_tenant, value_matches_policy_literal, PolicyExpr, PolicyLiteral,
    ReadPolicy, ReadPredicate,
};

use crate::{render::render_relation_subquery, SqlSink, SqlValue};

/// Render the combined `(NOT (deny_1 OR …)) AND (allow_1 OR …)` clause for
/// a single action. Returns `false` when the action has no allow policies
/// (in which case the caller should emit a literal `FALSE`, matching the
/// previous behaviour) — the function only writes when at least one allow
/// rule is present.
pub fn render_action_policy<S: SqlSink + ?Sized>(
    sink: &mut S,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) -> bool {
    if allow_policies.is_empty() {
        sink.push_sql("FALSE");
        return true;
    }

    if !deny_policies.is_empty() {
        sink.push_sql("NOT (");
        render_policy_disjunction(sink, deny_policies, ctx);
        sink.push_sql(") AND (");
        render_policy_disjunction(sink, allow_policies, ctx);
        sink.push_sql(")");
    } else {
        render_policy_disjunction(sink, allow_policies, ctx);
    }
    true
}

/// Render the read policy clause (allow + deny) used to scope SELECTs.
/// Differs from [`render_action_policy`] only in semantics: this is the
/// shape `cratestack-sqlx::render::render_read_policy_sql` previously
/// produced, where an empty allow set short-circuits to `FALSE`.
pub fn render_read_policy<S: SqlSink + ?Sized>(
    sink: &mut S,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    let _ = render_action_policy(sink, allow_policies, deny_policies, ctx);
}

/// Render a single policy expression. Public so callers that need to fold a
/// `PolicyExpr` into a larger clause (e.g. correlated subqueries) can reuse
/// the traversal.
pub fn render_policy_expr<S: SqlSink + ?Sized>(
    sink: &mut S,
    expr: PolicyExpr,
    ctx: &CoolContext,
) {
    match expr {
        PolicyExpr::Predicate(predicate) => render_predicate(sink, predicate, ctx),
        PolicyExpr::And(exprs) => render_policy_group(sink, exprs, " AND ", ctx),
        PolicyExpr::Or(exprs) => render_policy_group(sink, exprs, " OR ", ctx),
    }
}

/// Convert a `cratestack_core::Value` into a `SqlValue` for binding into a
/// policy clause. Returns `None` for non-scalar variants (List, Map, Bytes,
/// Null) which can't appear on either side of a column comparison. Public
/// because the backends use it when extracting auth values for the
/// `FieldEqAuth` / `FieldNeAuth` predicates.
pub fn auth_value_to_sql(ctx: &CoolContext, auth_field: &str) -> Option<SqlValue> {
    match ctx.auth_field(auth_field)? {
        Value::Bool(value) => Some(SqlValue::Bool(*value)),
        Value::Int(value) => Some(SqlValue::Int(*value)),
        Value::String(value) => Some(SqlValue::String(value.clone())),
        _ => None,
    }
}

fn render_policy_disjunction<S: SqlSink + ?Sized>(
    sink: &mut S,
    policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    if policies.is_empty() {
        sink.push_sql("FALSE");
        return;
    }
    for (index, policy) in policies.iter().enumerate() {
        if index > 0 {
            sink.push_sql(" OR ");
        }
        render_policy_expr(sink, policy.expr, ctx);
    }
}

fn render_policy_group<S: SqlSink + ?Sized>(
    sink: &mut S,
    exprs: &[PolicyExpr],
    joiner: &str,
    ctx: &CoolContext,
) {
    sink.push_sql("(");
    for (index, expr) in exprs.iter().enumerate() {
        if index > 0 {
            sink.push_sql(joiner);
        }
        render_policy_expr(sink, *expr, ctx);
    }
    sink.push_sql(")");
}

fn render_predicate<S: SqlSink + ?Sized>(
    sink: &mut S,
    predicate: ReadPredicate,
    ctx: &CoolContext,
) {
    match predicate {
        ReadPredicate::AuthNotNull => push_bool_literal(sink, ctx.is_authenticated()),
        ReadPredicate::AuthIsNull => push_bool_literal(sink, !ctx.is_authenticated()),
        ReadPredicate::HasRole { role } => push_bool_literal(sink, context_has_role(ctx, role)),
        ReadPredicate::InTenant { tenant_id } => {
            push_bool_literal(sink, context_in_tenant(ctx, tenant_id))
        }
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => push_bool_literal(
            sink,
            ctx.auth_field(auth_field)
                .is_some_and(|candidate| value_matches_policy_literal(candidate, value)),
        ),
        ReadPredicate::AuthFieldNeLiteral { auth_field, value } => push_bool_literal(
            sink,
            ctx.auth_field(auth_field)
                .is_some_and(|candidate| !value_matches_policy_literal(candidate, value)),
        ),
        ReadPredicate::FieldIsTrue { column } => {
            sink.push_sql(column);
            sink.push_sql(" = TRUE");
        }
        ReadPredicate::FieldEqLiteral { column, value } => {
            sink.push_sql(column);
            sink.push_sql(" = ");
            sink.push_bind(&policy_literal_to_sql(value));
        }
        ReadPredicate::FieldNeLiteral { column, value } => {
            sink.push_sql(column);
            sink.push_sql(" != ");
            sink.push_bind(&policy_literal_to_sql(value));
        }
        ReadPredicate::FieldEqAuth { column, auth_field } => {
            match auth_value_to_sql(ctx, auth_field) {
                Some(value) => {
                    sink.push_sql(column);
                    sink.push_sql(" = ");
                    sink.push_bind(&value);
                }
                None => sink.push_sql("FALSE"),
            }
        }
        ReadPredicate::FieldNeAuth { column, auth_field } => {
            match auth_value_to_sql(ctx, auth_field) {
                Some(value) => {
                    sink.push_sql(column);
                    sink.push_sql(" != ");
                    sink.push_bind(&value);
                }
                None => sink.push_sql("FALSE"),
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
            render_relation_subquery(
                sink,
                quantifier,
                parent_table,
                parent_column,
                related_table,
                related_column,
                &|sink| render_policy_expr(sink, *expr, ctx),
            );
        }
    }
}

fn push_bool_literal<S: SqlSink + ?Sized>(sink: &mut S, value: bool) {
    sink.push_sql(if value { "TRUE" } else { "FALSE" });
}

/// Convert a compile-time `PolicyLiteral` into the runtime `SqlValue` used
/// for binding.
pub fn policy_literal_to_sql(literal: PolicyLiteral) -> SqlValue {
    match literal {
        PolicyLiteral::Bool(value) => SqlValue::Bool(value),
        PolicyLiteral::Int(value) => SqlValue::Int(value),
        PolicyLiteral::String(value) => SqlValue::String(value.to_owned()),
    }
}
