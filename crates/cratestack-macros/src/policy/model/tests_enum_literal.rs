//! Macro-time coverage for required-enum-field literal comparisons in
//! `@@allow`/`@@deny` read policies (issue #666). Mirrors
//! `tests_system_principal`'s harness: parse a small fixture schema
//! string with the real parser, then lower its policy through
//! [`generate_policies_for_action`] and inspect the emitted tokens.
//!
//! Runtime SQL-filtering coverage (the "does the emitted predicate
//! actually restrict rows to the right variant" half) lives in
//! `crates/cratestack-pg/tests/policy_db_enum_literal.rs` — a compiling
//! test here only proves codegen accepts the syntax, not that the
//! generated `WHERE` clause is correct.

use super::generate_policies_for_action;

fn asset_schema(expression: &str) -> String {
    format!(
        r#"
enum AssetPurpose {{
  product_image
  kyc_document_front
}}

model Asset {{
  id Int @id
  purpose AssetPurpose

  @@allow("read", {expression})
}}
"#
    )
}

fn optional_asset_schema(expression: &str) -> String {
    format!(
        r#"
enum AssetPurpose {{
  product_image
  kyc_document_front
}}

model Asset {{
  id Int @id
  purpose AssetPurpose?

  @@allow("read", {expression})
}}
"#
    )
}

fn lower(schema: &str) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let parsed = cratestack_parser::parse_schema(schema).expect("fixture schema should parse");
    let model = parsed.models.first().expect("fixture declares a model");
    generate_policies_for_action(
        model,
        &parsed.models,
        &parsed.types,
        &parsed.enums,
        parsed.auth.as_ref(),
        "read",
    )
}

/// Decisive test: a required enum field compared against a bareword
/// variant literal must compile and lower to the existing
/// `FieldEqLiteral`/`PolicyLiteral::String` machinery — not a new,
/// parallel predicate shape.
#[test]
fn required_enum_field_equality_lowers_to_field_eq_literal() {
    let lowered = lower(&asset_schema("purpose == product_image"))
        .expect("required enum literal equality should compile")
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        lowered.contains("FieldEqLiteral"),
        "expected FieldEqLiteral predicate, got: {lowered}"
    );
    assert!(
        lowered.contains("PolicyLiteral :: String"),
        "expected the enum variant to lower to PolicyLiteral::String (enum columns are stored \
         as TEXT holding the variant name verbatim), got: {lowered}"
    );
    assert!(
        lowered.contains("\"product_image\""),
        "expected the literal variant name in the emitted tokens, got: {lowered}"
    );
}

/// `!=` on the same field must lower to `FieldNeLiteral`.
#[test]
fn required_enum_field_inequality_lowers_to_field_ne_literal() {
    let lowered = lower(&asset_schema("purpose != product_image"))
        .expect("required enum literal inequality should compile")
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        lowered.contains("FieldNeLiteral"),
        "expected FieldNeLiteral predicate, got: {lowered}"
    );
}

/// `field == A || field == B` — the supported substitute for `in`
/// against a set of variants (issue #666 explicitly scopes `in` out;
/// this is the existing `Or` combinator doing the same job).
#[test]
fn enum_equality_composes_with_or_for_multiple_variants() {
    let lowered = lower(&asset_schema(
        "purpose == product_image || purpose == kyc_document_front",
    ))
    .expect("composed enum literal equality should compile")
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    assert!(lowered.contains("Or"), "got: {lowered}");
    assert!(lowered.contains("\"product_image\""), "got: {lowered}");
    assert!(lowered.contains("\"kyc_document_front\""), "got: {lowered}");
}

/// An unknown variant name must be a compile error, not a silently
/// permissive predicate.
#[test]
fn unknown_variant_is_a_compile_error() {
    let result = lower(&asset_schema("purpose == not_a_real_variant"));
    assert!(
        result.is_err(),
        "unknown enum variant should not compile, got: {:?}",
        result.map(|tokens| tokens.iter().map(ToString::to_string).collect::<Vec<_>>())
    );
}

/// Decisive test: an OPTIONAL enum field must still be rejected, with
/// an error that names the actual problem (not required) rather than
/// falling back to the generic "unsupported field type" message.
#[test]
fn optional_enum_field_is_rejected_with_a_clear_error() {
    let result = lower(&optional_asset_schema("purpose == product_image"));
    let Err(message) = result else {
        panic!(
            "optional enum field literal comparison must not compile, got: {:?}",
            result.map(|tokens| tokens.iter().map(ToString::to_string).collect::<Vec<_>>())
        );
    };
    assert!(
        message.contains("purpose") && message.contains("required"),
        "expected a clear error naming the field and the required-arity requirement, got: {message}"
    );
}
