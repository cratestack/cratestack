//! Per-`ReadPredicate` SQL emission. Most predicates are evaluated
//! at render time against `ctx` and collapse to `TRUE`/`FALSE` SQL
//! constants; the field-comparison predicates emit one bind slot.

use std::fmt::Write;

use cratestack_core::CoolContext;
use cratestack_policy::{context_has_role, context_in_tenant};

use crate::query::{auth_value_to_sql, value_matches_auth_literal};
use crate::ReadPredicate;

use super::policy::render_relation_policy_sql;

pub(super) fn render_policy_predicate(
    predicate: ReadPredicate,
    ctx: &CoolContext,
    sql: &mut String,
    bind_index: &mut usize,
) {
    match predicate {
        ReadPredicate::AuthNotNull => {
            sql.push_str(if ctx.is_authenticated() { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::AuthIsNull => {
            sql.push_str(if ctx.is_authenticated() { "FALSE" } else { "TRUE" });
        }
        ReadPredicate::HasRole { role } => {
            sql.push_str(if context_has_role(ctx, role) { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::InTenant { tenant_id } => {
            sql.push_str(if context_in_tenant(ctx, tenant_id) { "TRUE" } else { "FALSE" });
        }
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => {
            sql.push_str(
                if ctx
                    .auth_field(auth_field)
                    .is_some_and(|c| value_matches_auth_literal(c, value))
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
                    .is_some_and(|c| !value_matches_auth_literal(c, value))
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
    }
}

