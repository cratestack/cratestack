//! Per-`ReadPredicate` pusher used by [`super::policy::push_policy_expr_query`].
//! Most predicates evaluate at render time against `ctx` and collapse
//! to a `TRUE`/`FALSE` SQL constant; comparison predicates emit one
//! bind slot.

use cratestack_core::CratestackContext;
use cratestack_policy::{context_has_role, context_in_tenant};

use crate::{PolicyLiteral, ReadPredicate, sqlx};

use super::policy_relation::push_relation_policy_query;
use super::values::{auth_value_to_sql, push_bind_value, value_matches_auth_literal};

pub(super) fn push_policy_predicate(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    predicate: ReadPredicate,
    ctx: &CratestackContext,
) {
    match predicate {
        ReadPredicate::AuthNotNull => {
            query.push(if ctx.is_authenticated() {
                "TRUE"
            } else {
                "FALSE"
            });
        }
        ReadPredicate::AuthIsNull => {
            query.push(if ctx.is_authenticated() {
                "FALSE"
            } else {
                "TRUE"
            });
        }
        ReadPredicate::AuthIsSystem => {
            query.push(if ctx.is_system() { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::HasRole { role } => {
            query.push(if context_has_role(ctx, role) {
                "TRUE"
            } else {
                "FALSE"
            });
        }
        ReadPredicate::InTenant { tenant_id } => {
            query.push(if context_in_tenant(ctx, tenant_id) {
                "TRUE"
            } else {
                "FALSE"
            });
        }
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => {
            query.push(
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
            query.push(
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
            query.push(column).push(" = TRUE");
        }
        ReadPredicate::FieldEqLiteral { column, value } => {
            query.push(column).push(" = ");
            push_policy_literal(query, value);
        }
        ReadPredicate::FieldNeLiteral { column, value } => {
            query.push(column).push(" != ");
            push_policy_literal(query, value);
        }
        ReadPredicate::FieldInLiterals { column, values } => {
            push_in_list(query, column, values, false);
        }
        ReadPredicate::FieldNotInLiterals { column, values } => {
            push_in_list(query, column, values, true);
        }
        ReadPredicate::FieldEqAuth { column, auth_field } => {
            if let Some(value) = auth_value_to_sql(ctx, auth_field) {
                query.push(column).push(" = ");
                push_bind_value(query, &value);
            } else {
                query.push("FALSE");
            }
        }
        ReadPredicate::FieldNeAuth { column, auth_field } => {
            if let Some(value) = auth_value_to_sql(ctx, auth_field) {
                query.push(column).push(" != ");
                push_bind_value(query, &value);
            } else {
                query.push("FALSE");
            }
        }
        ReadPredicate::Relation {
            quantifier,
            parent_table,
            parent_column,
            related_table,
            related_column,
            expr,
        } => push_relation_policy_query(
            query,
            quantifier,
            parent_table,
            parent_column,
            related_table,
            related_column,
            expr,
            ctx,
        ),
    }
}

/// `column IN (...)` with one bind per element (issue #666). Mirrors
/// `render::policy_predicate::render_in_list`'s slot count exactly —
/// the two must agree or `preview_scoped_sql` would misreport the
/// executed query's parameter numbering.
///
/// An empty `values` cannot come from a compiled schema (the macro
/// rejects `field in []`); it collapses to the constant it means rather
/// than emitting the invalid `IN ()`.
fn push_in_list(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    column: &str,
    values: &[PolicyLiteral],
    negate: bool,
) {
    if values.is_empty() {
        query.push(if negate { "TRUE" } else { "FALSE" });
        return;
    }
    query.push(column);
    query.push(if negate { " NOT IN (" } else { " IN (" });
    for (slot, literal) in values.iter().enumerate() {
        if slot > 0 {
            query.push(", ");
        }
        push_policy_literal(query, *literal);
    }
    query.push(")");
}

fn push_policy_literal(query: &mut sqlx::QueryBuilder<sqlx::Postgres>, literal: PolicyLiteral) {
    match literal {
        PolicyLiteral::Bool(value) => query.push_bind(value),
        PolicyLiteral::Int(value) => query.push_bind(value),
        PolicyLiteral::String(value) => query.push_bind(value.to_owned()),
    };
}
