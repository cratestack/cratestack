//! Procedure-policy evaluation entrypoints and helpers.

use cratestack_core::{CratestackContext, CratestackError, Value};

use crate::procedure_types::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral, ProcedurePredicate,
};

/// Evaluate a procedure-dialect policy. Deny-by-default: an empty
/// `allow_policies` refuses everyone.
pub fn authorize_procedure<A: ProcedureArgs + ?Sized>(
    allow_policies: &[ProcedurePolicy],
    deny_policies: &[ProcedurePolicy],
    args: &A,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    authorize_with_construct("procedure", allow_policies, deny_policies, args, ctx)
}

/// [`authorize_procedure`] for a `query` block (cratestack#867).
///
/// Identical evaluation — the dialect is shared, deliberately (design §6)
/// — with one difference that is not cosmetic: the refusal says "query
/// policy denied this operation". A schema author debugging a denied
/// `query` was previously told a *procedure* had refused them, which
/// sends them looking through a construct they may not have written.
pub fn authorize_query<A: ProcedureArgs + ?Sized>(
    allow_policies: &[ProcedurePolicy],
    deny_policies: &[ProcedurePolicy],
    args: &A,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    authorize_with_construct("query", allow_policies, deny_policies, args, ctx)
}

/// The single evaluator both entry points delegate to.
///
/// `construct` reaches the message only; it never affects the decision,
/// which is what keeps "a query and a procedure with the same policy
/// behave identically" true by construction rather than by review.
fn authorize_with_construct<A: ProcedureArgs + ?Sized>(
    construct: &str,
    allow_policies: &[ProcedurePolicy],
    deny_policies: &[ProcedurePolicy],
    args: &A,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    let denied = || CratestackError::Forbidden(format!("{construct} policy denied this operation"));

    if allow_policies.is_empty() {
        return Err(denied());
    }

    if deny_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        return Err(denied());
    }

    if allow_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        Ok(())
    } else {
        Err(denied())
    }
}

pub fn context_has_role(ctx: &CratestackContext, role: &str) -> bool {
    ctx.auth_field("role")
        .or_else(|| ctx.auth_field("actor.role"))
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == role))
}

pub fn context_in_tenant(ctx: &CratestackContext, tenant_id: &str) -> bool {
    ctx.auth_field("tenant.id")
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == tenant_id))
}

fn procedure_policy_expr_matches<A: ProcedureArgs + ?Sized>(
    expr: ProcedurePolicyExpr,
    args: &A,
    ctx: &CratestackContext,
) -> bool {
    match expr {
        ProcedurePolicyExpr::Predicate(predicate) => {
            procedure_predicate_matches(predicate, args, ctx)
        }
        ProcedurePolicyExpr::And(exprs) => exprs
            .iter()
            .copied()
            .all(|expr| procedure_policy_expr_matches(expr, args, ctx)),
        ProcedurePolicyExpr::Or(exprs) => exprs
            .iter()
            .copied()
            .any(|expr| procedure_policy_expr_matches(expr, args, ctx)),
    }
}

fn procedure_predicate_matches<A: ProcedureArgs + ?Sized>(
    predicate: ProcedurePredicate,
    args: &A,
    ctx: &CratestackContext,
) -> bool {
    match predicate {
        ProcedurePredicate::Literal(value) => value,
        ProcedurePredicate::AuthNotNull => ctx.is_authenticated(),
        ProcedurePredicate::AuthIsNull => !ctx.is_authenticated(),
        ProcedurePredicate::AuthIsSystem => ctx.is_system(),
        ProcedurePredicate::HasRole { role } => context_has_role(ctx, role),
        ProcedurePredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id),
        ProcedurePredicate::AuthFieldEqLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| value_matches_literal(candidate, value)),
        ProcedurePredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| !value_matches_literal(candidate, value)),
        ProcedurePredicate::InputFieldIsTrue { field } => args
            .procedure_arg_value(field)
            .is_some_and(|value| value == Value::Bool(true)),
        ProcedurePredicate::InputFieldEqLiteral { field, value } => args
            .procedure_arg_value(field)
            .is_some_and(|candidate| value_matches_literal(&candidate, value)),
        ProcedurePredicate::InputFieldNeLiteral { field, value } => args
            .procedure_arg_value(field)
            .is_some_and(|candidate| !value_matches_literal(&candidate, value)),
        ProcedurePredicate::InputFieldEqAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(left), Some(right)) => &left == right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldNeAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(left), Some(right)) => &left != right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldEqInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldNeInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => left != right,
                _ => false,
            }
        }
    }
}

fn value_matches_literal(value: &Value, literal: ProcedurePolicyLiteral) -> bool {
    match (value, literal) {
        (Value::Bool(left), ProcedurePolicyLiteral::Bool(right)) => *left == right,
        (Value::Int(left), ProcedurePolicyLiteral::Int(right)) => *left == right,
        (Value::String(left), ProcedurePolicyLiteral::String(right)) => left == right,
        _ => false,
    }
}
