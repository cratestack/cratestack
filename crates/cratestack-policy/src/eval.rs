//! Procedure-policy evaluation entrypoints and helpers.

use cratestack_core::{CoolContext, CoolError, Value};

use crate::procedure_types::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral,
    ProcedurePredicate,
};

pub fn authorize_procedure<A: ProcedureArgs + ?Sized>(
    allow_policies: &[ProcedurePolicy],
    deny_policies: &[ProcedurePolicy],
    args: &A,
    ctx: &CoolContext,
) -> Result<(), CoolError> {
    if allow_policies.is_empty() {
        return Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ));
    }

    if deny_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        return Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ));
    }

    if allow_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        Ok(())
    } else {
        Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ))
    }
}

pub fn context_has_role(ctx: &CoolContext, role: &str) -> bool {
    ctx.auth_field("role")
        .or_else(|| ctx.auth_field("actor.role"))
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == role))
}

pub fn context_in_tenant(ctx: &CoolContext, tenant_id: &str) -> bool {
    ctx.auth_field("tenant.id")
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == tenant_id))
}

fn procedure_policy_expr_matches<A: ProcedureArgs + ?Sized>(
    expr: ProcedurePolicyExpr,
    args: &A,
    ctx: &CoolContext,
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
    ctx: &CoolContext,
) -> bool {
    match predicate {
        ProcedurePredicate::AuthNotNull => ctx.is_authenticated(),
        ProcedurePredicate::AuthIsNull => !ctx.is_authenticated(),
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
