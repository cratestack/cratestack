#![cfg(test)]

//! A `BigInt` auth claim above 2^53 has to be a string to survive a
//! JavaScript issuer. A procedure argument of type `BigInt` is always a
//! `Value::Int`. Derived equality called `Value::String("7")` different from
//! `Value::Int(7)`, so `owner != auth().accountId` was TRUE for a caller whose
//! claim was the string `"7"`, and a policy meant to refuse that caller let
//! them through.
//!
//! The string is not read as a number: an integer and a string are
//! undecidable, and an undecidable comparison satisfies neither `==` nor `!=`
//! (the rule is in `crate::compare`, the same as the SQL path's
//! `cratestack-sqlx` `query/support/comparison.rs`). This file is the
//! operator matrix at the three values that matter: a small one, the first
//! integer a JS number cannot hold, and `i64::MAX`. The other pairs and the
//! logic around an undecidable comparison are in
//! `tests_claim_comparison_edges.rs` and `tests_claim_comparison_unchanged.rs`.

use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral,
    ProcedurePredicate, authorize_procedure,
};
use cratestack_core::{CratestackContext, Value};
use std::collections::BTreeMap;

pub(crate) struct MapArgs(pub(crate) BTreeMap<&'static str, Value>);

impl ProcedureArgs for MapArgs {
    fn procedure_arg_value(&self, field: &str) -> Option<Value> {
        self.0.get(field).cloned()
    }
}

pub(crate) fn args(owner: Value) -> MapArgs {
    MapArgs(BTreeMap::from([("owner", owner)]))
}

pub(crate) fn claim(value: Value) -> CratestackContext {
    CratestackContext::authenticated([("accountId".to_owned(), value)])
}

pub(crate) fn policy(predicate: ProcedurePredicate) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(predicate),
    }
}

/// `@allow(auth().accountId == <n>)`.
pub(crate) fn eq_literal(n: i64) -> ProcedurePolicy {
    policy(ProcedurePredicate::AuthFieldEqLiteral {
        auth_field: "accountId",
        value: ProcedurePolicyLiteral::Int(n),
    })
}

/// `@allow(auth().accountId != <n>)`.
pub(crate) fn ne_literal(n: i64) -> ProcedurePolicy {
    policy(ProcedurePredicate::AuthFieldNeLiteral {
        auth_field: "accountId",
        value: ProcedurePolicyLiteral::Int(n),
    })
}

/// `@allow(owner == auth().accountId)`.
pub(crate) fn eq_auth() -> ProcedurePolicy {
    policy(ProcedurePredicate::InputFieldEqAuth {
        field: "owner",
        auth_field: "accountId",
    })
}

/// `@allow(owner != auth().accountId)`.
pub(crate) fn ne_auth() -> ProcedurePolicy {
    policy(ProcedurePredicate::InputFieldNeAuth {
        field: "owner",
        auth_field: "accountId",
    })
}

/// `(n, a different number next to it)`. `2^53 + 1` is paired with `2^53`, the
/// number an `f64` round trip would turn it into.
pub(crate) const NUMBERS: [(i64, i64); 3] = [
    (7, 8),
    (9_007_199_254_740_993, 9_007_199_254_740_992),
    (i64::MAX, i64::MAX - 1),
];

pub(crate) fn allowed(
    allow: &[ProcedurePolicy],
    deny: &[ProcedurePolicy],
    owner: Value,
    ctx: &CratestackContext,
) -> bool {
    authorize_procedure(allow, deny, &args(owner), ctx).is_ok()
}

#[test]
fn an_integer_claim_decides_equality_and_inequality_with_a_literal() {
    for (n, other) in NUMBERS {
        let ctx = claim(Value::Int(n));
        assert!(
            allowed(&[eq_literal(n)], &[], Value::Null, &ctx),
            "{n} == {n}"
        );
        assert!(
            !allowed(&[eq_literal(other)], &[], Value::Null, &ctx),
            "{n} == {other}"
        );
        assert!(
            !allowed(&[ne_literal(n)], &[], Value::Null, &ctx),
            "{n} != {n}"
        );
        assert!(
            allowed(&[ne_literal(other)], &[], Value::Null, &ctx),
            "{n} != {other}"
        );
    }
}

#[test]
fn an_integer_claim_decides_equality_and_inequality_with_an_argument() {
    for (n, other) in NUMBERS {
        let ctx = claim(Value::Int(n));
        assert!(
            allowed(&[eq_auth()], &[], Value::Int(n), &ctx),
            "{n} == {n}"
        );
        assert!(
            !allowed(&[eq_auth()], &[], Value::Int(other), &ctx),
            "{other} == {n}"
        );
        assert!(
            !allowed(&[ne_auth()], &[], Value::Int(n), &ctx),
            "{n} != {n}"
        );
        assert!(
            allowed(&[ne_auth()], &[], Value::Int(other), &ctx),
            "{other} != {n}"
        );
    }
}

/// The defect: a string claim passed `!=` against its own value. It passes
/// neither `!=` nor `==`, against the value it spells or any other.
#[test]
fn a_string_claim_satisfies_neither_equality_nor_inequality_with_a_literal() {
    for (n, other) in NUMBERS {
        let ctx = claim(Value::String(n.to_string()));
        for literal in [n, other] {
            for (name, policy) in [("==", eq_literal(literal)), ("!=", ne_literal(literal))] {
                assert!(
                    !allowed(&[policy], &[], Value::Null, &ctx),
                    "\"{n}\" {name} {literal} must deny"
                );
            }
        }
    }
}

/// The shape the issue names: `args.owner != auth().id`.
#[test]
fn a_string_claim_satisfies_neither_equality_nor_inequality_with_an_argument() {
    for (n, other) in NUMBERS {
        let ctx = claim(Value::String(n.to_string()));
        for owner in [n, other] {
            for (name, policy) in [("==", eq_auth()), ("!=", ne_auth())] {
                assert!(
                    !allowed(&[policy], &[], Value::Int(owner), &ctx),
                    "{owner} {name} \"{n}\" must deny"
                );
            }
        }
    }
}
