//! `BigInt` in model policy literals: `field == 7`, `field != 7` and
//! `field in [..]` (ADR 0019, PR B).

use super::literal::parse_policy_literal;
use crate::shared::test_support::{builtin_scalars, field};
use cratestack_core::TypeArity;

fn literal(rhs: &str, ty: &str, arity: TypeArity) -> Result<String, String> {
    parse_policy_literal(rhs, &field("f", ty, arity), &[]).map(|tokens| tokens.to_string())
}

#[test]
fn a_bigint_literal_is_a_policy_integer_literal() {
    assert_eq!(
        literal("7", "BigInt", TypeArity::Required).unwrap(),
        ":: cratestack :: PolicyLiteral :: Int (7i64)"
    );
}

#[test]
fn a_bigint_literal_beyond_2_pow_53_keeps_every_digit() {
    // 2^53 + 1: the value a JavaScript number cannot hold, and so the
    // smallest literal that would expose an `f64` round trip in this path.
    let rendered = literal("9007199254740993", "BigInt", TypeArity::Required).unwrap();
    assert!(rendered.contains("9007199254740993i64"), "{rendered}");
    let rendered = literal("-9223372036854775808", "BigInt", TypeArity::Required).unwrap();
    assert!(
        rendered.contains("- 9223372036854775808i64")
            || rendered.contains("-9223372036854775808i64"),
        "{rendered}"
    );
}

#[test]
fn a_bigint_literal_outside_i64_is_refused_at_expansion() {
    for rhs in [
        "9223372036854775808",
        "-9223372036854775809",
        "1.5",
        "seven",
        "\"7\"",
    ] {
        let error = literal(rhs, "BigInt", TypeArity::Required).unwrap_err();
        assert!(error.contains("expected integer literal"), "{rhs}: {error}");
    }
}

#[test]
fn an_optional_bigint_field_has_no_literal_form() {
    let error = literal("7", "BigInt", TypeArity::Optional).unwrap_err();
    assert!(
        error.contains("BigInt"),
        "the refusal names the supported scalars: {error}"
    );
}

#[test]
fn the_literal_table_accepts_exactly_the_scalars_it_documents() {
    // Guard: which built-in scalars can appear as a policy literal. A new
    // scalar must be a decision (add an arm, or leave it refused here).
    let supported = ["Boolean", "Int", "BigInt", "String"];
    for scalar in builtin_scalars() {
        let rhs = match scalar {
            "Boolean" => "true",
            "String" => "\"x\"",
            _ => "7",
        };
        let accepted = literal(rhs, scalar, TypeArity::Required).is_ok();
        assert_eq!(
            accepted,
            supported.contains(&scalar),
            "`{scalar}`: literal policy support changed; update the table and the error text"
        );
    }
}
