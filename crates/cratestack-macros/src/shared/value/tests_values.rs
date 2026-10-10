//! `value_tokens` for `BigInt`, and the guard on its `Value::Null` fallback
//! (ADR 0019, PR B, risk 2).
//!
//! The fallback is a fail-open for a negated policy: `args.owner != 7` is
//! `!value_matches_literal(Value::Null, Int(7))`, which is `true`, so a
//! `BigInt` argument that lowered to `Null` was allowed through by exactly
//! the rule meant to refuse it. The generated code compiled, and nothing
//! pointed at the arm.

use std::collections::BTreeSet;

use cratestack_core::TypeArity;
use quote::quote;

use super::value_tokens;
use crate::shared::test_support::{builtin_scalars, render, type_ref};

const NULL: &str = ":: cratestack :: Value :: Null";

fn tokens(scalar: &str, arity: TypeArity) -> String {
    render(&value_tokens(
        quote! { self.field.clone() },
        &type_ref(scalar, arity),
        &BTreeSet::new(),
    ))
}

#[test]
fn a_required_bigint_is_its_i64_never_null() {
    assert_eq!(
        tokens("BigInt", TypeArity::Required),
        ":: cratestack :: Value :: Int (self . field . clone () . get ())"
    );
}

#[test]
fn an_optional_bigint_is_its_i64_or_null_when_absent() {
    let rendered = tokens("BigInt", TypeArity::Optional);
    assert!(
        rendered.contains(":: cratestack :: Value :: Int (value . get ())"),
        "{rendered}"
    );
    assert!(
        rendered.contains("None => :: cratestack :: Value :: Null"),
        "{rendered}"
    );
    assert!(
        !rendered.trim_start().starts_with(NULL),
        "a present value must not collapse to Null: {rendered}"
    );
}

#[test]
fn a_bigint_list_is_a_list_of_i64_values() {
    let rendered = tokens("BigInt", TypeArity::List);
    assert!(
        rendered.contains(":: cratestack :: Value :: List"),
        "{rendered}"
    );
    assert!(
        rendered.contains(":: cratestack :: Value :: Int (value . get ())"),
        "{rendered}"
    );
    assert!(!rendered.contains(NULL), "{rendered}");
}

/// Built-in scalars a procedure policy can compare to a literal or to
/// another argument by value.
const COMPARABLE: [&str; 5] = ["String", "Cuid", "Int", "BigInt", "Boolean"];

/// Built-in scalars with no policy-comparable form: they lower to
/// `Value::Null` on purpose, and a policy cannot name one in a literal.
const NULL_ON_PURPOSE: [&str; 9] = [
    "Float",
    "DateTime",
    "Decimal",
    "Json",
    "Bytes",
    "Uuid",
    "Vector",
    "Geography",
    "Geometry",
];

#[test]
fn every_builtin_scalar_is_comparable_or_null_on_purpose() {
    for scalar in builtin_scalars() {
        let comparable = COMPARABLE.contains(&scalar);
        let null_on_purpose = NULL_ON_PURPOSE.contains(&scalar);
        assert!(
            comparable ^ null_on_purpose,
            "`{scalar}` is in neither (or both) of COMPARABLE and NULL_ON_PURPOSE: a new built-in \
             scalar reaches the `Value::Null` fallback of `value_tokens` until it is given an arm \
             or named here as deliberately incomparable"
        );
        for arity in [TypeArity::Required, TypeArity::Optional] {
            let rendered = tokens(scalar, arity);
            if comparable {
                assert!(
                    rendered.contains("Value :: Int")
                        || rendered.contains("Value :: String")
                        || rendered.contains("Value :: Bool"),
                    "`{scalar}` ({arity:?}) must produce a real value, got: {rendered}"
                );
            } else {
                assert_eq!(rendered, NULL, "`{scalar}` ({arity:?})");
            }
        }
    }
}
