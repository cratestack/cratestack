//! `BigInt` in procedure policies (ADR 0019, PR B, risk 2): the literal a
//! `BigInt` argument is compared with, and that it can be named at all.

use cratestack_core::{Schema, TypeArity};

use super::{ProcedurePolicyField, parse_procedure_literal};
use crate::policy::procedure::{PolicySubject, generate_procedure_policy};
use crate::shared::test_support::{builtin_scalars, type_ref};

const SCHEMA: &str = r#"
type Reply {
  ok Boolean
}

procedure transfer(owner: BigInt, other: BigInt): Reply
  @allow(owner != 7)
"#;

fn schema() -> Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

fn literal(rhs: &str, scalar: &str, arity: TypeArity) -> Result<String, String> {
    let schema = schema();
    let subject = PolicySubject::procedure(&schema.procedures[0]);
    let field = ProcedurePolicyField {
        ty: type_ref(scalar, arity),
    };
    parse_procedure_literal(rhs, Some(&field), "owner", &subject).map(|tokens| tokens.to_string())
}

#[test]
fn a_bigint_argument_compares_with_an_integer_policy_literal() {
    assert_eq!(
        literal("7", "BigInt", TypeArity::Required).unwrap(),
        ":: cratestack :: ProcedurePolicyLiteral :: Int (7i64)"
    );
}

#[test]
fn a_bigint_literal_keeps_every_digit_above_2_pow_53() {
    let rendered = literal("9007199254740993", "BigInt", TypeArity::Required).unwrap();
    assert!(rendered.contains("9007199254740993i64"), "{rendered}");
}

#[test]
fn a_bigint_literal_outside_i64_or_not_an_integer_is_refused() {
    for rhs in ["9223372036854775808", "7.5", "\"7\"", "seven"] {
        let error = literal(rhs, "BigInt", TypeArity::Required).unwrap_err();
        assert!(error.contains("expected integer literal"), "{rhs}: {error}");
    }
}

#[test]
fn only_a_required_bigint_has_a_literal_form() {
    for arity in [TypeArity::Optional, TypeArity::List] {
        let error = literal("7", "BigInt", arity).unwrap_err();
        assert!(error.contains("BigInt"), "{arity:?}: {error}");
    }
}

#[test]
fn the_procedure_literal_table_accepts_exactly_the_scalars_it_documents() {
    // Guard on the `_ => Err(..)` arm: a new built-in scalar must be a
    // decision here, as `BigInt` was, not an accidental refusal or accept.
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
            "`{scalar}`: procedure literal support changed; update the table and the error text"
        );
    }
}

#[test]
fn a_negated_bigint_comparison_lowers_with_a_literal_not_a_null_check() {
    // The whole policy, end to end through the generator: `owner != 7`
    // on a `BigInt` argument is an `InputFieldNeLiteral` with an integer
    // literal. Evaluated against `Value::Int(7)` that denies; against the
    // `Value::Null` the old fallback produced it allowed.
    let schema = schema();
    let subject = PolicySubject::procedure(&schema.procedures[0]);
    let rendered =
        generate_procedure_policy("owner != 7", &subject, &schema.types, schema.auth.as_ref())
            .expect("a BigInt argument can be named in a policy")
            .to_string();
    assert!(rendered.contains("InputFieldNeLiteral"), "{rendered}");
    assert!(
        rendered.contains("ProcedurePolicyLiteral :: Int (7i64)"),
        "{rendered}"
    );
    assert!(rendered.contains("field : \"owner\""), "{rendered}");
}

#[test]
fn two_bigint_arguments_compare_by_value() {
    let schema = schema();
    let subject = PolicySubject::procedure(&schema.procedures[0]);
    let rendered = generate_procedure_policy(
        "owner == other",
        &subject,
        &schema.types,
        schema.auth.as_ref(),
    )
    .expect("two BigInt arguments share a type")
    .to_string();
    assert!(rendered.contains("InputFieldEqInput"), "{rendered}");
}
