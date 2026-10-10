#![cfg(test)]

//! Pairs the integer-against-string rule does not touch. They keep the answer
//! they had before it, so a policy that compares no integer with a string is
//! not moved in either direction: `==` is derived equality and `!=` is its
//! negation, with no third outcome. This pins current behaviour, not a
//! judgement that it is right (a `Bool` claim against an integer literal
//! passes `!=`; `cratestack-sqlx` refuses that pair, see `crate::compare`).

use crate::tests_claim_comparison::{
    MapArgs, allowed, claim, eq_auth, eq_literal, ne_auth, ne_literal, policy,
};
use crate::{ProcedurePolicyLiteral, ProcedurePredicate, authorize_procedure};
use cratestack_core::Value;
use std::collections::BTreeMap;

fn string(text: &str) -> Value {
    Value::String(text.to_owned())
}

#[test]
fn a_claim_that_is_not_a_string_or_an_integer_keeps_its_answer_against_an_integer() {
    // (claim, literal, equal)
    let literals: [(Value, i64, bool); 5] = [
        (Value::Int(7), 7, true),
        (Value::Int(7), 8, false),
        (Value::Bool(true), 7, false),
        (Value::Float(7.0), 7, false),
        (Value::Null, 7, false),
    ];
    for (claim_value, literal, equal) in literals {
        let ctx = claim(claim_value.clone());
        assert_eq!(
            allowed(&[eq_literal(literal)], &[], Value::Null, &ctx),
            equal,
            "{claim_value:?} == {literal}"
        );
        assert_eq!(
            allowed(&[ne_literal(literal)], &[], Value::Null, &ctx),
            !equal,
            "{claim_value:?} != {literal}"
        );
    }
}

#[test]
fn string_against_string_is_an_exact_comparison() {
    // A claim against an argument: "007" is not "7".
    let ctx = claim(string("007"));
    assert!(allowed(&[eq_auth()], &[], string("007"), &ctx));
    assert!(!allowed(&[ne_auth()], &[], string("007"), &ctx));
    assert!(!allowed(&[eq_auth()], &[], string("7"), &ctx));
    assert!(allowed(&[ne_auth()], &[], string("7"), &ctx));

    // A claim against a string literal.
    let literal = |value: &'static str, negate: bool| {
        let (auth_field, value) = ("accountId", ProcedurePolicyLiteral::String(value));
        policy(if negate {
            ProcedurePredicate::AuthFieldNeLiteral { auth_field, value }
        } else {
            ProcedurePredicate::AuthFieldEqLiteral { auth_field, value }
        })
    };
    let ctx = claim(string("7"));
    assert!(allowed(&[literal("7", false)], &[], Value::Null, &ctx));
    assert!(!allowed(&[literal("7", true)], &[], Value::Null, &ctx));
    assert!(allowed(&[literal("8", true)], &[], Value::Null, &ctx));
    assert!(!allowed(&[literal("8", false)], &[], Value::Null, &ctx));
}

/// Two arguments are compared with each other. The macro has already required
/// them to share a type, so an integer never meets a string here from
/// generated code; a hand-written policy gets the same refusal as everywhere.
#[test]
fn two_arguments_follow_the_same_rule() {
    let ctx = claim(Value::Null);
    let pair = |left: Value, right: Value| MapArgs(BTreeMap::from([("a", left), ("b", right)]));
    let eq = [policy(ProcedurePredicate::InputFieldEqInput {
        field: "a",
        other_field: "b",
    })];
    let ne = [policy(ProcedurePredicate::InputFieldNeInput {
        field: "a",
        other_field: "b",
    })];

    let same = pair(Value::Int(7), Value::Int(7));
    assert!(authorize_procedure(&eq, &[], &same, &ctx).is_ok());
    assert!(authorize_procedure(&ne, &[], &same, &ctx).is_err());
    let different = pair(string("x"), string("y"));
    assert!(authorize_procedure(&eq, &[], &different, &ctx).is_err());
    assert!(authorize_procedure(&ne, &[], &different, &ctx).is_ok());

    let mixed = pair(Value::Int(7), string("7"));
    assert!(authorize_procedure(&eq, &[], &mixed, &ctx).is_err());
    assert!(authorize_procedure(&ne, &[], &mixed, &ctx).is_err());
}
