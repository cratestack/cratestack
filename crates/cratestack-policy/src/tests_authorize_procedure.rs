#![cfg(test)]

//! Coverage for [`authorize_procedure`]'s core precedence rules
//! (default-deny, deny-beats-allow) and the `Literal`/`AuthNotNull`/
//! `AuthIsNull` predicates. See `tests_procedure_context_predicates.rs`
//! for `HasRole`/`InTenant`/`AuthField*` plus the `context_has_role`/
//! `context_in_tenant` helpers directly, and
//! `tests_procedure_input_predicates.rs` for the `InputField*`
//! variants (which need a real `ProcedureArgs` impl instead of the
//! `NoArgs` stub below).

use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePredicate, authorize_procedure,
};
use cratestack_core::{CoolContext, CoolError, Value};

struct NoArgs;
impl ProcedureArgs for NoArgs {
    fn procedure_arg_value(&self, _field: &str) -> Option<Value> {
        None
    }
}

fn policy(predicate: ProcedurePredicate) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(predicate),
    }
}

fn literal_policy(value: bool) -> ProcedurePolicy {
    policy(ProcedurePredicate::Literal(value))
}

/// `@allow(true)` (`ProcedurePredicate::Literal(true)`) must authorize
/// every caller, including an unauthenticated one — the exact "public
/// procedure" case a bare `true` clause is meant to express (see
/// `ProcedurePredicate::Literal`'s docs). Before this variant existed,
/// the only way to express "public" was two `@allow` clauses covering
/// `auth() == null` / `auth() != null`; this proves the direct spelling
/// is behaviourally equivalent for the case that matters.
#[test]
fn literal_true_allows_unauthenticated_callers() {
    let unauthenticated = CoolContext::anonymous();
    assert!(!unauthenticated.is_authenticated());
    let result = authorize_procedure(&[literal_policy(true)], &[], &NoArgs, &unauthenticated);
    assert!(result.is_ok(), "expected @allow(true) to allow: {result:?}");
}

/// `@allow(true)` must also allow an authenticated caller — it is
/// unconditional, not merely "unauthenticated is fine too".
#[test]
fn literal_true_allows_authenticated_callers() {
    let authenticated = CoolContext::authenticated([]);
    let result = authorize_procedure(&[literal_policy(true)], &[], &NoArgs, &authenticated);
    assert!(result.is_ok(), "expected @allow(true) to allow: {result:?}");
}

/// `@deny(true)` (`ProcedurePredicate::Literal(true)` in a deny clause)
/// must refuse unconditionally, mirroring `@allow(true)`'s unconditional
/// accept.
#[test]
fn literal_true_in_deny_refuses_unconditionally() {
    let ctx = CoolContext::authenticated([]);
    let result = authorize_procedure(
        &[literal_policy(true)],
        &[literal_policy(true)],
        &NoArgs,
        &ctx,
    );
    assert!(matches!(result, Err(CoolError::Forbidden(_))));
}

/// `@allow(false)` never matches, so with no other `@allow` clause the
/// procedure is unconditionally closed — same outcome as an empty
/// `ALLOW_POLICIES` list, reached a different way.
#[test]
fn literal_false_never_allows() {
    let ctx = CoolContext::authenticated([]);
    let result = authorize_procedure(&[literal_policy(false)], &[], &NoArgs, &ctx);
    assert!(matches!(result, Err(CoolError::Forbidden(_))));
}

/// An empty `allow_policies` list must deny unconditionally — the
/// literal default-deny case, distinct from `literal_false_never_allows`
/// (which has an allow clause that merely never matches). No procedure
/// should ever compile to an empty `ALLOW_POLICIES` in practice (schema
/// validation requires at least one `@allow`), but the evaluator itself
/// must not silently permit that degenerate case.
#[test]
fn empty_allow_policies_default_denies() {
    let ctx = CoolContext::authenticated([]);
    let result = authorize_procedure(&[], &[], &NoArgs, &ctx);
    assert!(matches!(result, Err(CoolError::Forbidden(_))));

    let anonymous = CoolContext::anonymous();
    let result = authorize_procedure(&[], &[], &NoArgs, &anonymous);
    assert!(matches!(result, Err(CoolError::Forbidden(_))));
}

/// Deny-beats-allow precedence using non-trivial (non-literal)
/// predicates on both sides: an admin who matches `@allow(hasRole(...))`
/// is still refused once a matching `@deny` fires, proving deny isn't
/// merely "skip this allow clause" but an unconditional veto over the
/// whole allow set.
#[test]
fn deny_beats_allow_precedence_with_real_predicates() {
    let admin =
        CoolContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
    let allow = [policy(ProcedurePredicate::HasRole { role: "admin" })];
    let deny = [policy(ProcedurePredicate::AuthNotNull)];

    let result = authorize_procedure(&allow, &deny, &NoArgs, &admin);
    assert!(
        matches!(result, Err(CoolError::Forbidden(_))),
        "deny should veto even though the allow clause matches: {result:?}"
    );
}

#[test]
fn auth_not_null_and_auth_is_null_variants() {
    let authenticated = CoolContext::authenticated([]);
    let anonymous = CoolContext::anonymous();

    assert!(
        authorize_procedure(
            &[policy(ProcedurePredicate::AuthNotNull)],
            &[],
            &NoArgs,
            &authenticated
        )
        .is_ok()
    );
    assert!(
        authorize_procedure(
            &[policy(ProcedurePredicate::AuthNotNull)],
            &[],
            &NoArgs,
            &anonymous
        )
        .is_err()
    );
    assert!(
        authorize_procedure(
            &[policy(ProcedurePredicate::AuthIsNull)],
            &[],
            &NoArgs,
            &anonymous
        )
        .is_ok()
    );
    assert!(
        authorize_procedure(
            &[policy(ProcedurePredicate::AuthIsNull)],
            &[],
            &NoArgs,
            &authenticated
        )
        .is_err()
    );
}
