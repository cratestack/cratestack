#![cfg(test)]

//! The comparison rule of `crate::compare`, one pair at a time.

use crate::compare::{Comparison, compare_literal, compare_values};
use crate::procedure_types::ProcedurePolicyLiteral;
use crate::truth::Truth;
use cratestack_core::Value;

/// A spread of values, every kind of `Value` among them.
fn samples() -> Vec<Value> {
    vec![
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::Int(0),
        Value::Int(7),
        Value::Int(-7),
        Value::Int(i64::MAX),
        Value::Float(7.0),
        Value::Float(f64::NAN),
        Value::String(String::new()),
        Value::String("7".to_owned()),
        Value::String("007".to_owned()),
        Value::String("+7".to_owned()),
        Value::String("abc".to_owned()),
        Value::Bytes(vec![7]),
        Value::List(vec![Value::Int(7)]),
        Value::Map([("a".to_owned(), Value::Int(7))].into()),
    ]
}

fn is_int_against_string(left: &Value, right: &Value) -> bool {
    matches!(
        (left, right),
        (Value::Int(_), Value::String(_)) | (Value::String(_), Value::Int(_))
    )
}

/// An integer against a string is undecidable, in either order, whatever the
/// string spells.
#[test]
fn an_integer_against_a_string_is_undecidable_in_either_order() {
    for left in samples() {
        for right in samples() {
            if is_int_against_string(&left, &right) {
                assert_eq!(
                    compare_values(&left, &right),
                    Comparison::Undecidable,
                    "{left:?} {right:?}"
                );
            }
        }
    }
}

/// Every other pair is derived equality and never undecidable: the guard that
/// the rule cannot move a policy that compares no integer with a string.
#[test]
fn every_other_pair_is_derived_equality() {
    for left in samples() {
        for right in samples() {
            if is_int_against_string(&left, &right) {
                continue;
            }
            let expected = if left == right {
                Comparison::Equal
            } else {
                Comparison::Different
            };
            assert_eq!(
                compare_values(&left, &right),
                expected,
                "{left:?} {right:?}"
            );
        }
    }
}

#[test]
fn a_literal_of_the_other_kind_is_undecidable_and_a_literal_of_the_same_kind_is_decided() {
    use ProcedurePolicyLiteral::{Bool, Int, String as Str};
    let string = |text: &str| Value::String(text.to_owned());
    for (value, literal, expected) in [
        (Value::Int(7), Int(7), Comparison::Equal),
        (Value::Int(7), Int(8), Comparison::Different),
        (string("7"), Str("7"), Comparison::Equal),
        (string("7"), Str("8"), Comparison::Different),
        (Value::Bool(true), Bool(true), Comparison::Equal),
        (Value::Bool(true), Bool(false), Comparison::Different),
        (string("7"), Int(7), Comparison::Undecidable),
        (string("+7"), Int(7), Comparison::Undecidable),
        (string(""), Int(0), Comparison::Undecidable),
        (Value::Int(7), Str("7"), Comparison::Undecidable),
        // Unchanged: no third outcome for these.
        (Value::Bool(true), Int(1), Comparison::Different),
        (Value::Float(7.0), Int(7), Comparison::Different),
        (Value::Null, Str("7"), Comparison::Different),
    ] {
        assert_eq!(
            compare_literal(&value, literal),
            expected,
            "{value:?} {literal:?}"
        );
    }
}

#[test]
fn comparison_maps_to_truth_with_undecidable_unknown_for_both_operators() {
    assert_eq!(Comparison::Equal.for_eq(), Truth::True);
    assert_eq!(Comparison::Equal.for_ne(), Truth::False);
    assert_eq!(Comparison::Different.for_eq(), Truth::False);
    assert_eq!(Comparison::Different.for_ne(), Truth::True);
    assert_eq!(Comparison::Undecidable.for_eq(), Truth::Unknown);
    assert_eq!(Comparison::Undecidable.for_ne(), Truth::Unknown);
}
