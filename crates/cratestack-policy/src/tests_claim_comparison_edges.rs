#![cfg(test)]

//! The edges of the integer-against-string rule: every kind of string, both
//! directions, a string literal, and the way an undecidable comparison travels
//! through `&&`, `||`, `@allow` and `@deny`.
//!
//! The operator matrix at 7, 2^53 + 1 and `i64::MAX` is in
//! `tests_claim_comparison.rs`; the pairs the rule leaves alone are in
//! `tests_claim_comparison_unchanged.rs`.

use crate::tests_claim_comparison::{
    allowed, args, claim, eq_auth, eq_literal, ne_auth, ne_literal, policy,
};
use crate::{
    ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral, ProcedurePredicate,
    authorize_procedure,
};
use cratestack_core::Value;

/// A canonical decimal, a leading sign, leading zeros, a fraction, whitespace,
/// an empty string, a word, and a canonical string past `i64`. None of them is
/// a number to the evaluator, so none is equal to one or different from one.
const STRINGS: [&str; 10] = [
    "7",
    "+7",
    "007",
    "7.0",
    " 7",
    "-0",
    "",
    "seven",
    "9223372036854775807",
    "9223372036854775808",
];

#[test]
fn no_string_claim_satisfies_equality_or_inequality_with_an_integer() {
    for text in STRINGS {
        let ctx = claim(Value::String(text.to_owned()));
        for number in [0, 7, 8, i64::MAX] {
            for policy in [eq_literal(number), ne_literal(number)] {
                assert!(
                    !allowed(&[policy], &[], Value::Null, &ctx),
                    "{text:?} vs {number}"
                );
            }
            for policy in [eq_auth(), ne_auth()] {
                assert!(
                    !allowed(&[policy], &[], Value::Int(number), &ctx),
                    "{number} vs {text:?}"
                );
            }
        }
    }
}

/// The other direction: an integer claim against a string, whether the string
/// is an argument or a literal.
#[test]
fn an_integer_claim_satisfies_neither_equality_nor_inequality_with_a_string() {
    let ctx = claim(Value::Int(7));
    for text in STRINGS {
        for policy in [eq_auth(), ne_auth()] {
            assert!(
                !allowed(&[policy], &[], Value::String(text.to_owned()), &ctx),
                "{text:?} vs 7"
            );
        }
    }
    let literal = |value: &'static str, negate: bool| {
        let (auth_field, value) = ("accountId", ProcedurePolicyLiteral::String(value));
        policy(if negate {
            ProcedurePredicate::AuthFieldNeLiteral { auth_field, value }
        } else {
            ProcedurePredicate::AuthFieldEqLiteral { auth_field, value }
        })
    };
    for text in ["7", "8", "+7", ""] {
        for negate in [false, true] {
            assert!(
                !allowed(&[literal(text, negate)], &[], Value::Null, &ctx),
                "7 vs literal {text:?}, negated {negate}"
            );
        }
    }
}

/// A `@deny` on an undecidable comparison fires, whichever operator it uses.
#[test]
fn a_deny_fires_on_every_string_claim() {
    let open = [policy(ProcedurePredicate::Literal(true))];
    for text in STRINGS {
        let ctx = claim(Value::String(text.to_owned()));
        for deny in [eq_auth(), ne_auth()] {
            assert!(!allowed(&open, &[deny], Value::Int(7), &ctx), "{text:?}");
        }
        for deny in [eq_literal(7), ne_literal(7)] {
            assert!(!allowed(&open, &[deny], Value::Null, &ctx), "{text:?}");
        }
    }
}

fn or(exprs: &'static [ProcedurePolicyExpr]) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::Or(exprs),
    }
}

fn and(exprs: &'static [ProcedurePolicyExpr]) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::And(exprs),
    }
}

const UNKNOWN: ProcedurePolicyExpr =
    ProcedurePolicyExpr::Predicate(ProcedurePredicate::AuthFieldNeLiteral {
        auth_field: "accountId",
        value: ProcedurePolicyLiteral::Int(7),
    });
const TRUE: ProcedurePolicyExpr = ProcedurePolicyExpr::Predicate(ProcedurePredicate::Literal(true));
const FALSE: ProcedurePolicyExpr =
    ProcedurePolicyExpr::Predicate(ProcedurePredicate::Literal(false));

/// With the claim `"7"`, `UNKNOWN` above is undecidable. It is not true, so
/// it never grants; it is not false either, so it never lets a conjunction or
/// a deny pretend it was decided.
#[test]
fn an_undecidable_comparison_follows_three_valued_logic() {
    let ctx = claim(Value::String("7".to_owned()));
    let run = |allow: &[ProcedurePolicy], deny: &[ProcedurePolicy]| {
        authorize_procedure(allow, deny, &args(Value::Null), &ctx).is_ok()
    };
    let open = || policy(ProcedurePredicate::Literal(true));

    // An allow needs TRUE.
    assert!(!run(&[or(&[UNKNOWN, FALSE])], &[]));
    assert!(!run(&[and(&[UNKNOWN, TRUE])], &[]));
    // Another branch that is TRUE still grants.
    assert!(run(&[or(&[UNKNOWN, TRUE])], &[]));
    // FALSE decides a conjunction whatever is beside it, so a deny made of
    // `UNKNOWN && FALSE` does not fire.
    assert!(run(&[open()], &[and(&[UNKNOWN, FALSE])]));
    // A deny fires unless it is FALSE.
    assert!(!run(&[open()], &[or(&[UNKNOWN, FALSE])]));
    assert!(!run(&[open()], &[and(&[UNKNOWN, TRUE])]));
    assert!(!run(&[open()], &[or(&[UNKNOWN, TRUE])]));
    // An empty `And` is TRUE and an empty `Or` is FALSE, as before.
    assert!(run(&[and(&[])], &[]));
    assert!(!run(&[or(&[])], &[]));
}

/// The same policy with an integer claim is decided, which is what makes the
/// string-claim answers above a property of the claim and not of the policy.
#[test]
fn the_same_expressions_are_decided_for_an_integer_claim() {
    let ctx = claim(Value::Int(8));
    let run = |allow: &[ProcedurePolicy], deny: &[ProcedurePolicy]| {
        authorize_procedure(allow, deny, &args(Value::Null), &ctx).is_ok()
    };
    assert!(run(&[or(&[UNKNOWN, FALSE])], &[]));
    assert!(run(&[and(&[UNKNOWN, TRUE])], &[]));
    assert!(!run(&[or(&[FALSE])], &[]));
}
