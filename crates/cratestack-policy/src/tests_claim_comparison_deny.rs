#![cfg(test)]

//! A `@deny` on a claim comparison: it fires when the comparison is true or
//! undecidable, and stays silent only when it is false. The operator matrix is
//! in `tests_claim_comparison.rs`.

use crate::ProcedurePredicate;
use crate::tests_claim_comparison::{
    NUMBERS, allowed, claim, eq_auth, eq_literal, ne_auth, ne_literal, policy,
};
use cratestack_core::Value;

/// An undecidable comparison is not false, so a `@deny` built on it fires:
/// spelling the claim as a string does not walk around the refusal.
#[test]
fn a_deny_fires_on_a_string_claim() {
    let open = [policy(ProcedurePredicate::Literal(true))];
    for (n, other) in NUMBERS {
        let ctx = claim(Value::String(n.to_string()));
        for owner in [n, other] {
            for deny in [eq_auth(), ne_auth()] {
                assert!(
                    !allowed(&open, &[deny], Value::Int(owner), &ctx),
                    "{owner} vs \"{n}\""
                );
            }
        }
        for deny in [
            eq_literal(n),
            ne_literal(n),
            eq_literal(other),
            ne_literal(other),
        ] {
            assert!(
                !allowed(&open, &[deny], Value::Null, &ctx),
                "\"{n}\" vs a literal"
            );
        }
    }
}

/// The control for the test above: an integer claim leaves a `@deny` that
/// does not match silent.
#[test]
fn a_deny_stays_silent_on_an_integer_claim_that_does_not_match() {
    let open = [policy(ProcedurePredicate::Literal(true))];
    for (n, other) in NUMBERS {
        let ctx = claim(Value::Int(n));
        assert!(!allowed(&open, &[eq_auth()], Value::Int(n), &ctx));
        assert!(allowed(&open, &[eq_auth()], Value::Int(other), &ctx));
        assert!(allowed(&open, &[ne_auth()], Value::Int(n), &ctx));
        assert!(!allowed(&open, &[ne_auth()], Value::Int(other), &ctx));
    }
}
