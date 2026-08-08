#![cfg(test)]

//! SPIKE (`spike/b1-internal-actions`): runtime semantics of the
//! `auth().isSystem()` policy predicate.
//!
//! Exercises the real evaluator — `render_read_policy_sql`, the same
//! entry point `tests_read_policy_predicates.rs` uses — so these are
//! assertions about what SQL the policy compiles to for a given
//! caller, not about a re-implementation. No DB connection needed.
//!
//! The design being validated: a system principal is something
//! policies **name**, not something callers **skip**. Consequently the
//! most important test in this file is
//! [`model_that_never_names_is_system_denies_system_callers`] — the
//! fail-closed case. If that one ever goes green for the wrong reason,
//! the feature has degenerated into the blanket bypass flag it was
//! designed not to be.

use crate::{PolicyExpr, ReadPolicy, ReadPredicate, render::render_read_policy_sql};
use cratestack_core::{CoolContext, SystemContext, Value};

fn render(allow: &[ReadPolicy], ctx: &CoolContext) -> String {
    let mut bind_index = 1usize;
    render_read_policy_sql(allow, &[], ctx, &mut bind_index).expect("policy should render")
}

fn allow(expr: PolicyExpr) -> [ReadPolicy; 1] {
    [ReadPolicy { expr }]
}

fn system_ctx() -> CoolContext {
    SystemContext::for_service("device-reconciler").into_context()
}

fn user_ctx(subject_id: &str) -> CoolContext {
    CoolContext::authenticated([("subjectId".to_owned(), Value::String(subject_id.to_owned()))])
}

/// The shape a downstream schema would actually write:
/// `@@allow("update", auth().isSystem() || subjectId == auth().subjectId)`.
fn owner_or_system() -> PolicyExpr {
    PolicyExpr::Or(&[
        PolicyExpr::Predicate(ReadPredicate::AuthIsSystem),
        PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
            column: "subject_id",
            auth_field: "subjectId",
        }),
    ])
}

/// (b), granting half: a system caller satisfies the `isSystem()` arm
/// unconditionally — it collapses to `TRUE`, so no row filter is
/// applied on top of it and server code can touch any row.
#[test]
fn is_system_grants_a_system_context_caller() {
    let sql = render(&allow(owner_or_system()), &system_ctx());
    assert_eq!(sql, "(TRUE OR FALSE)");
}

/// (b), denying half: the *same policy* for a request-derived caller
/// collapses the `isSystem()` arm to `FALSE` and falls through to the
/// ownership check, which still binds the caller's own subject id.
/// The end user is not elevated by the mere presence of the arm.
#[test]
fn is_system_denies_a_request_derived_caller() {
    let sql = render(&allow(owner_or_system()), &user_ctx("subject-1"));
    assert_eq!(sql, "(FALSE OR subject_id = $1)");

    // Anonymous callers get neither arm.
    let sql = render(&allow(owner_or_system()), &CoolContext::anonymous());
    assert_eq!(sql, "(FALSE OR FALSE)");
}

/// (c) THE FAIL-CLOSED CASE — the most important test in this spike.
///
/// A model whose policies never mention `isSystem()` must not become
/// writable just because the caller holds a `SystemContext`. This is
/// the property that distinguishes "a principal policies name" from "a
/// bypass flag": the system context carries no `subjectId` claim, so
/// the ownership predicate renders `FALSE` for it, exactly as it would
/// for any other caller missing that claim.
#[test]
fn model_that_never_names_is_system_denies_system_callers() {
    let owner_only = allow(PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
        column: "subject_id",
        auth_field: "subjectId",
    }));

    let sql = render(&owner_only, &system_ctx());
    assert_eq!(
        sql, "FALSE",
        "a system caller must gain nothing on a model that never names isSystem()"
    );

    // Contrast: the identical policy does admit a real owner, so the
    // FALSE above is the policy denying the system caller, not the
    // policy being broken outright.
    let sql = render(&owner_only, &user_ctx("subject-1"));
    assert_eq!(sql, "subject_id = $1");
}

/// The other fail-closed direction: a model with *no* policy for an
/// action is not writable by system code either. Default-deny is
/// unconditional and the system principal does not weaken it.
#[test]
fn empty_allow_list_still_denies_system_callers() {
    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&[], &[], &system_ctx(), &mut bind_index)
        .expect("empty allow should still render a clause");
    assert_eq!(sql, "FALSE");
}

/// A `@@deny` rule outranks the system principal, same as it outranks
/// everything else. `isSystem()` is not an escape hatch from deny.
#[test]
fn deny_rules_still_beat_a_system_caller() {
    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(
        &allow(PolicyExpr::Predicate(ReadPredicate::AuthIsSystem)),
        &allow(PolicyExpr::Predicate(ReadPredicate::AuthIsSystem)),
        &system_ctx(),
        &mut bind_index,
    )
    .expect("policy should render");
    assert_eq!(sql, "NOT (TRUE) AND (TRUE)");
}

/// The create path has its own in-process evaluator rather than SQL
/// pushdown, so `AuthIsSystem` has to be wired there too. Same three
/// cases, through `evaluate_input_predicate`.
mod create_path {
    use super::*;
    use crate::query::evaluate_input_predicate_for_tests as evaluate;

    #[test]
    fn is_system_predicate_matches_only_system_contexts() {
        assert!(evaluate(ReadPredicate::AuthIsSystem, &[], &system_ctx()));
        assert!(!evaluate(
            ReadPredicate::AuthIsSystem,
            &[],
            &user_ctx("subject-1")
        ));
        assert!(!evaluate(
            ReadPredicate::AuthIsSystem,
            &[],
            &CoolContext::anonymous()
        ));
    }

    /// Fail-closed on the create path: a system caller does not
    /// satisfy an ownership predicate it was never named in.
    #[test]
    fn system_caller_does_not_satisfy_an_ownership_predicate() {
        assert!(!evaluate(
            ReadPredicate::FieldEqAuth {
                column: "subject_id",
                auth_field: "subjectId",
            },
            &[],
            &system_ctx()
        ));
    }
}
