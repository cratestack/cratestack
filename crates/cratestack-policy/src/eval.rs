//! Procedure-policy evaluation entrypoints and helpers.

use cratestack_core::{CratestackContext, CratestackError, Value};

use crate::predicate::procedure_predicate_truth;
use crate::procedure_types::{ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr};
use crate::truth::Truth;

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

    // A `@deny` stays silent only when its expression is definitely false. A
    // comparison that cannot be decided (a claim that is not a number against
    // a number, see `crate::compare`) is not false, so the deny fires.
    if deny_policies
        .iter()
        .any(|policy| !procedure_policy_expr_truth(policy.expr, args, ctx).is_false())
    {
        return Err(denied());
    }

    // An `@allow` grants only when its expression is definitely true.
    if allow_policies
        .iter()
        .any(|policy| procedure_policy_expr_truth(policy.expr, args, ctx).is_true())
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

fn procedure_policy_expr_truth<A: ProcedureArgs + ?Sized>(
    expr: ProcedurePolicyExpr,
    args: &A,
    ctx: &CratestackContext,
) -> Truth {
    match expr {
        ProcedurePolicyExpr::Predicate(predicate) => {
            procedure_predicate_truth(predicate, args, ctx)
        }
        ProcedurePolicyExpr::And(exprs) => Truth::all(
            exprs
                .iter()
                .map(|expr| procedure_policy_expr_truth(*expr, args, ctx)),
        ),
        ProcedurePolicyExpr::Or(exprs) => Truth::any(
            exprs
                .iter()
                .map(|expr| procedure_policy_expr_truth(*expr, args, ctx)),
        ),
    }
}
