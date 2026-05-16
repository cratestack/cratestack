//! Per-`ReadPredicate` pusher used by [`super::policy::push_policy_expr_query`].
//! Most predicates evaluate at render time against `ctx` and collapse
//! to a `TRUE`/`FALSE` SQL constant; comparison predicates emit one
//! bind slot.

use cratestack_core::CoolContext;
use cratestack_policy::{context_has_role, context_in_tenant};

use crate::{PolicyLiteral, ReadPredicate, sqlx};

use super::policy_relation::push_relation_policy_query;
use super::values::{auth_value_to_sql, push_bind_value, value_matches_auth_literal};

pub(super) fn push_policy_predicate(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    predicate: ReadPredicate,
    ctx: &CoolContext,
) {
    match predicate {
        ReadPredicate::AuthNotNull => {
            query.push(if ctx.is_authenticated() { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::AuthIsNull => {
            query.push(if ctx.is_authenticated() { "FALSE" } else { "TRUE" });
        }
        ReadPredicate::HasRole { role } => {
            query.push(if context_has_role(ctx, role) { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::InTenant { tenant_id } => {
            query.push(if context_in_tenant(ctx, tenant_id) { "TRUE" } else { "FALSE" });
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

fn push_policy_literal(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    literal: PolicyLiteral,
) {
    match literal {
        PolicyLiteral::Bool(value) => query.push_bind(value),
        PolicyLiteral::Int(value) => query.push_bind(value),
        PolicyLiteral::String(value) => query.push_bind(value.to_owned()),
    };
}
