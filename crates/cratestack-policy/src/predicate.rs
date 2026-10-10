//! One predicate of a procedure policy, evaluated against the procedure's
//! arguments and the caller's context.
//!
//! Every predicate is a [`Truth`]. Only a comparison of an integer with a
//! string can be `Unknown` (see [`crate::compare`]); the rest are `True` or
//! `False` exactly as they were when this returned a `bool`. A claim or an
//! argument that is absent is `False` for `==` and for `!=` alike, so a
//! missing value never satisfies either.

use cratestack_core::{CratestackContext, Value};

use crate::compare::{compare_literal, compare_values};
use crate::eval::{context_has_role, context_in_tenant};
use crate::procedure_types::{ProcedureArgs, ProcedurePredicate};
use crate::truth::Truth;

pub(crate) fn procedure_predicate_truth<A: ProcedureArgs + ?Sized>(
    predicate: ProcedurePredicate,
    args: &A,
    ctx: &CratestackContext,
) -> Truth {
    match predicate {
        ProcedurePredicate::Literal(value) => value.into(),
        ProcedurePredicate::AuthNotNull => ctx.is_authenticated().into(),
        ProcedurePredicate::AuthIsNull => (!ctx.is_authenticated()).into(),
        ProcedurePredicate::AuthIsSystem => ctx.is_system().into(),
        ProcedurePredicate::HasRole { role } => context_has_role(ctx, role).into(),
        ProcedurePredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id).into(),
        ProcedurePredicate::AuthFieldEqLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .map_or(Truth::False, |claim| compare_literal(claim, value).for_eq()),
        ProcedurePredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .map_or(Truth::False, |claim| compare_literal(claim, value).for_ne()),
        ProcedurePredicate::InputFieldIsTrue { field } => args
            .procedure_arg_value(field)
            .is_some_and(|value| value == Value::Bool(true))
            .into(),
        ProcedurePredicate::InputFieldEqLiteral { field, value } => args
            .procedure_arg_value(field)
            .map_or(Truth::False, |arg| compare_literal(&arg, value).for_eq()),
        ProcedurePredicate::InputFieldNeLiteral { field, value } => args
            .procedure_arg_value(field)
            .map_or(Truth::False, |arg| compare_literal(&arg, value).for_ne()),
        ProcedurePredicate::InputFieldEqAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(arg), Some(claim)) => compare_values(&arg, claim).for_eq(),
                _ => Truth::False,
            }
        }
        ProcedurePredicate::InputFieldNeAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(arg), Some(claim)) => compare_values(&arg, claim).for_ne(),
                _ => Truth::False,
            }
        }
        ProcedurePredicate::InputFieldEqInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => compare_values(&left, &right).for_eq(),
                _ => Truth::False,
            }
        }
        ProcedurePredicate::InputFieldNeInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => compare_values(&left, &right).for_ne(),
                _ => Truth::False,
            }
        }
    }
}
