//! `@range` on a `BigInt` field (ADR 0019, PR B, risk 3).
//!
//! The old `emit_range` ended in `_ => quote! {}`: the parser accepted
//! `@range` on a `BigInt`, the generated `validate()` skipped it, and an
//! out-of-range value reached the database. Nothing failed to compile.

use std::collections::BTreeSet;

use cratestack_core::Schema;
use quote::quote;

use super::emit_range;
use crate::validators::{Validating, generate_input_validate_body, generate_type_validate_impl};

fn range(scalar: &str) -> String {
    emit_range(&quote! { "f" }, scalar, Some(0), Some(100)).to_string()
}

#[test]
fn bigint_range_checks_its_i64_against_the_declared_bounds() {
    assert_eq!(
        emit_range(&quote! { "totalE8" }, "BigInt", Some(0), Some(1_000_000)).to_string(),
        ":: cratestack :: validate_range_i64 (\"totalE8\" , value . get () , Some (0i64) , Some (1000000i64)) ? ;"
    );
}

#[test]
fn an_open_bound_stays_open() {
    let rendered = emit_range(&quote! { "f" }, "BigInt", None, Some(5)).to_string();
    assert!(rendered.contains("None , Some (5i64)"), "{rendered}");
}

#[test]
fn a_range_on_the_other_numeric_scalars_is_unchanged() {
    assert!(range("Int").contains("validate_range_i64 (\"f\" , * value"));
    assert!(range("Decimal").contains("validate_range_decimal"));
}

const SCHEMA: &str = r#"
model Account {
  id BigInt @id
  totalE8 BigInt @range(min: 0, max: 1000000)
  limitE8 BigInt? @range(min: 0)
  @@allow("all", auth() != null)
}

type Quote {
  amountE8 BigInt @range(min: 1)
}

type Reply {
  ok Boolean
}

procedure quote(args: Quote): Reply
  @allow(true)
"#;

fn schema() -> Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

#[test]
fn a_model_input_validates_a_bigint_range_in_both_arities() {
    let schema = schema();
    let fields: Vec<_> = schema.models[0].fields.iter().collect();
    let body = generate_input_validate_body(&fields, false)
        .expect("a @range on a BigInt is a validator")
        .to_string();
    assert!(
        body.contains("validate_range_i64 (\"totalE8\" , value . get ()"),
        "{body}"
    );
    // The nullable field is checked only when it holds a value.
    assert!(
        body.contains("validate_range_i64 (\"limitE8\" , value . get ()"),
        "{body}"
    );
    assert!(body.contains("if let Some (value)"), "{body}");

    let patch = generate_input_validate_body(&fields, true)
        .expect("update input")
        .to_string();
    assert!(
        patch.contains("Some (Some (value))"),
        "an update's nullable BigInt unwraps twice: {patch}"
    );
}

#[test]
fn a_type_field_validates_a_bigint_range_under_its_request_path() {
    let schema = schema();
    let validating = Validating::of(&schema.types, &schema.models);
    assert!(
        validating.contains("Quote"),
        "a BigInt @range makes the type validating"
    );
    let rendered = generate_type_validate_impl(&schema.types[0], &validating).to_string();
    assert!(
        rendered.contains("validate_range_i64 (& path . field (\"amountE8\") , value . get ()"),
        "{rendered}"
    );
}

#[test]
fn every_scalar_the_parser_accepts_a_range_on_has_a_generated_check() {
    // Guard on the catch-all. The parser's `@range` gate and `emit_range` are
    // two lists; when the first admits a scalar the second lacks, the
    // attribute is accepted and silently enforces nothing. Read the first
    // from the parser itself rather than copying it here.
    let mut accepted = BTreeSet::new();
    for scalar in crate::shared::test_support::builtin_scalars() {
        let param = if scalar == "Vector" {
            "Vector(3)"
        } else {
            scalar
        };
        let source = format!(
            "model M {{\n  id Int @id\n  f {param} @range(min: 0, max: 9)\n  @@allow(\"all\", auth() != null)\n}}\n"
        );
        if cratestack_parser::parse_schema(&source).is_ok() {
            accepted.insert(scalar);
        }
    }
    assert_eq!(
        accepted,
        BTreeSet::from(["Int", "BigInt", "Decimal"]),
        "the parser's @range gate changed; give the new scalar an arm in emit_range"
    );
    for scalar in accepted {
        assert!(
            range(scalar).contains("validate_range"),
            "`{scalar}` is accepted by @range but emit_range generates no check: {}",
            range(scalar)
        );
    }
}

#[test]
fn a_scalar_with_no_range_check_is_refused_at_expansion_not_skipped() {
    let rendered = range("Float");
    assert!(rendered.contains("compile_error"), "{rendered}");
    assert!(rendered.contains("Float"), "{rendered}");
}
