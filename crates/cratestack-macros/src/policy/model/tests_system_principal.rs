//! Compile-time lowering of `auth().isSystem()` (issue #486 / ADR 0038
//! blocker B1).
//!
//! The runtime semantics are covered in
//! `cratestack-sqlx/src/tests_system_principal_policy.rs`. What is
//! checked here is the half that can only be checked at macro time:
//! that the term reaches `ReadPredicate::AuthIsSystem` at all (it sits
//! in front of `parse_builtin_policy_call`, which would otherwise claim
//! it and error — see `term::is_auth_is_system_term`'s doc comment),
//! and — the fail-closed half — that a policy which never writes
//! `isSystem()` never emits that predicate.

use super::generate_policies_for_action;

fn device_schema(expression: &str) -> String {
    format!(
        r#"
auth Principal {{
  subjectId String
}}

model Device {{
  id Int @id
  subjectId String

  @@allow("update", {expression})
}}
"#
    )
}

fn lower(schema: &str) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let parsed = cratestack_parser::parse_schema(schema).expect("fixture schema should parse");
    let model = parsed.models.first().expect("fixture declares a model");
    generate_policies_for_action(
        model,
        &parsed.models,
        &parsed.types,
        parsed.auth.as_ref(),
        "update",
    )
}

fn lowered_update_policy(expression: &str) -> String {
    lower(&device_schema(expression))
        .expect("policy should compile")
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn auth_is_system_lowers_to_the_dedicated_predicate() {
    let lowered = lowered_update_policy("auth().isSystem()");
    assert!(
        lowered.contains("AuthIsSystem"),
        "expected AuthIsSystem predicate, got: {lowered}"
    );
}

/// The realistic schema shape: system code OR the record's owner.
#[test]
fn is_system_composes_with_an_ownership_check() {
    let lowered = lowered_update_policy("auth().isSystem() || subjectId == auth().subjectId");
    assert!(lowered.contains("AuthIsSystem"), "got: {lowered}");
    assert!(lowered.contains("FieldEqAuth"), "got: {lowered}");
    assert!(lowered.contains("Or"), "got: {lowered}");
}

/// Fail-closed at the codegen layer: nothing about an ordinary policy
/// implicitly acquires the system predicate. Without this, the runtime
/// fail-closed test could pass while codegen quietly injected an
/// `AuthIsSystem` arm into every model.
#[test]
fn policy_that_never_names_is_system_emits_no_system_predicate() {
    let lowered = lowered_update_policy("subjectId == auth().subjectId");
    assert!(
        !lowered.contains("AuthIsSystem"),
        "a policy that never names isSystem() must not emit the predicate, got: {lowered}"
    );

    let lowered = lowered_update_policy("auth() != null");
    assert!(!lowered.contains("AuthIsSystem"), "got: {lowered}");
}

/// Whitespace tolerance — the AST splitter only trims term edges, so
/// the recogniser normalizes interior whitespace itself.
#[test]
fn interior_whitespace_is_tolerated() {
    assert!(lowered_update_policy("auth() . isSystem ()").contains("AuthIsSystem"));
}

/// A near-miss must not silently succeed. `isAdmin()` is not a builtin
/// and must remain a compile error rather than degrading to something
/// permissive.
#[test]
fn unknown_auth_method_is_still_an_error() {
    let result = lower(&device_schema("auth().isAdmin()"));
    assert!(
        result.is_err(),
        "auth().isAdmin() should not compile, got: {:?}",
        result.map(|tokens| tokens.iter().map(ToString::to_string).collect::<Vec<_>>())
    );
}
