#![cfg(test)]

//! Coverage for the read-policy data types (`ReadPolicy`, `PolicyExpr`,
//! `ReadPredicate`, `RelationQuantifier`, `PolicyLiteral`) defined in
//! `read_types.rs`.
//!
//! **Why this file only checks construction/equality, not behavior:**
//! unlike `procedure_types.rs` (evaluated in-process by
//! `authorize_procedure` in `eval.rs`), `read_types.rs` defines pure
//! data with NO evaluator anywhere in this crate. Read policies are
//! compiled straight to SQL by consuming crates instead — see
//! `cratestack-sqlx`'s `render_read_policy_sql` (string rendering) and
//! `push_action_policy_query` (`QueryBuilder` rendering), both under
//! `crates/cratestack-sqlx/src/query/support/` and
//! `crates/cratestack-sqlx/src/render/`. The actual read-policy
//! evaluation-semantics coverage this crate's own `#[cfg(test)]`
//! surface can't provide — default-deny, deny-beats-allow precedence,
//! every `RelationQuantifier` variant (including the empty-relation
//! edge case), field-level denial, and every `ReadPredicate` variant —
//! lives in `crates/cratestack-sqlx/src/tests_read_policy_predicates.rs`
//! and `tests_read_policy_field_predicates.rs`, which exercise that
//! real evaluator with no database required (pure SQL-string
//! generation). This file exists so `read_types.rs` isn't left with a
//! zero `#[cfg(test)]` footprint, and to make the split above
//! discoverable from the type definitions themselves rather than only
//! from a design doc.

use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, RelationQuantifier};

/// Every `RelationQuantifier` variant must remain distinct under
/// equality — the evaluator (`render_relation_policy_sql`) branches on
/// this enum to choose between `EXISTS`, `NOT EXISTS`, and `NOT EXISTS
/// (... AND NOT (...))`, so an accidental `PartialEq`/`derive` change
/// that collapsed two variants together would silently change which
/// quantifier a schema's `some`/`every`/`none`/`toOne` compiles to.
#[test]
fn relation_quantifier_variants_are_pairwise_distinct() {
    let variants = [
        RelationQuantifier::ToOne,
        RelationQuantifier::Some,
        RelationQuantifier::Every,
        RelationQuantifier::None,
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            assert_eq!(
                a == b,
                i == j,
                "variants at {i} and {j} should only be equal to themselves"
            );
        }
    }
}

/// `PolicyLiteral` must round-trip through equality per-kind, and NOT
/// compare equal across kinds even when the underlying bit pattern
/// could coincide (e.g. `Bool(true)` vs `Int(1)`) — `value_matches_literal`
/// in the consuming evaluator match on `(Value, PolicyLiteral)` pairs by
/// variant, so a stray cross-kind `PartialEq` would be a real policy
/// bug (a column holding `1` could then match a schema literal `true`).
#[test]
fn policy_literal_equality_is_scoped_to_its_own_kind() {
    assert_eq!(PolicyLiteral::Bool(true), PolicyLiteral::Bool(true));
    assert_ne!(PolicyLiteral::Bool(true), PolicyLiteral::Bool(false));
    assert_eq!(PolicyLiteral::Int(1), PolicyLiteral::Int(1));
    assert_ne!(PolicyLiteral::Int(1), PolicyLiteral::Int(2));
    assert_eq!(PolicyLiteral::String("a"), PolicyLiteral::String("a"));
    assert_ne!(PolicyLiteral::String("a"), PolicyLiteral::String("b"));
}

/// A `ReadPolicy` wrapping a `PolicyExpr::And`/`Or` of `ReadPredicate`
/// leaves — the same shape schema-macro codegen emits for a
/// multi-clause `@@allow`/`@@deny` — must construct and compare as
/// expected. This is a smoke test that the recursive `PolicyExpr`
/// shape (used throughout `cratestack-sqlx`'s evaluator, including for
/// the nested-relation case in `tests_nested_relation_policy.rs`)
/// nests the way the type signature promises.
#[test]
fn policy_expr_and_or_nest_over_predicate_leaves() {
    static LEAVES: [PolicyExpr; 2] = [
        PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
        PolicyExpr::Predicate(ReadPredicate::FieldIsTrue {
            column: "published",
        }),
    ];
    let policy = ReadPolicy {
        expr: PolicyExpr::Or(&LEAVES),
    };
    match policy.expr {
        PolicyExpr::Or(exprs) => assert_eq!(exprs.len(), 2),
        other => panic!("expected Or, got {other:?}"),
    }

    static AND_LEAVES: [PolicyExpr; 1] = [PolicyExpr::Predicate(ReadPredicate::AuthNotNull)];
    let and_policy = ReadPolicy {
        expr: PolicyExpr::And(&AND_LEAVES),
    };
    match and_policy.expr {
        PolicyExpr::And(exprs) => assert_eq!(exprs.len(), 1),
        other => panic!("expected And, got {other:?}"),
    }
}
