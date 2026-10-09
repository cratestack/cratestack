use std::collections::BTreeSet;

use cratestack_core::SourceSpan;

use super::*;

fn synthetic_span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn vector_type_ref(arity: TypeArity, dimension: u32) -> TypeRef {
    TypeRef {
        name: "Vector".to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: Vec::new(),
        int_args: vec![dimension],
        ident_args: Vec::new(),
    }
}

#[test]
fn required_vector_field_maps_to_sql_value_vector() {
    let ty = vector_type_ref(TypeArity::Required, 1536);
    let enum_names: BTreeSet<&str> = BTreeSet::new();
    let tokens = sql_value_tokens(quote::quote! { self.embedding.clone() }, &ty, &enum_names);
    let rendered = tokens.to_string();
    assert!(
        rendered.contains("SqlValue :: Vector"),
        "rendered was: {rendered}"
    );
}

#[test]
fn optional_vector_field_maps_to_null_vector() {
    let ty = vector_type_ref(TypeArity::Optional, 3);
    let enum_names: BTreeSet<&str> = BTreeSet::new();
    let tokens = sql_value_tokens(quote::quote! { value }, &ty, &enum_names);
    let rendered = tokens.to_string();
    assert!(
        rendered.contains("SqlValue :: Vector") && rendered.contains("NullVector"),
        "rendered was: {rendered}"
    );
}

fn tokens(scalar: &str, arity: TypeArity) -> String {
    let ty = crate::shared::test_support::type_ref(scalar, arity);
    sql_value_tokens(quote::quote! { value }, &ty, &BTreeSet::new()).to_string()
}

#[test]
fn required_bigint_binds_its_i64_through_the_bigint_variant() {
    assert_eq!(
        tokens("BigInt", TypeArity::Required),
        ":: cratestack :: SqlValue :: BigInt (value . get ())"
    );
}

#[test]
fn optional_bigint_is_bigint_or_null_bigint_never_int() {
    let rendered = tokens("BigInt", TypeArity::Optional);
    assert!(
        rendered.contains("Some (value) => :: cratestack :: SqlValue :: BigInt (value . get ())"),
        "{rendered}"
    );
    assert!(
        rendered.contains("None => :: cratestack :: SqlValue :: NullBigInt"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("SqlValue :: Int") && !rendered.contains("NullInt"),
        "an Int variant would make an `INT8` column compare against an `Int`-typed operand: {rendered}"
    );
}

#[test]
fn create_and_update_inputs_wrap_a_bigint_field_in_the_same_variant() {
    let field = crate::shared::test_support::field("totalE8", "BigInt", TypeArity::Required);
    let enums = BTreeSet::new();
    let create = create_sql_value(&field, &enums).to_string();
    assert!(
        create.contains("column : \"total_e8\"")
            && create.contains("SqlValue :: BigInt (self . totalE8 . clone () . get ())"),
        "{create}"
    );
    let update = update_sql_value(&field, &enums).to_string();
    assert!(
        update.contains("SqlValue :: BigInt (value . get ())"),
        "{update}"
    );
}

#[test]
fn no_builtin_scalar_reaches_the_unsupported_value_panic() {
    // Guard: `sql_value_tokens` ends in `panic!("unsupported SQLx value type")`.
    // A scalar the parser accepts but this table forgot would abort the whole
    // macro expansion; a test that calls it for every built-in scalar turns
    // that into a failure here rather than in a consumer's build.
    for scalar in crate::shared::test_support::builtin_scalars() {
        for arity in [TypeArity::Required, TypeArity::Optional] {
            let rendered = tokens(scalar, arity);
            assert!(
                rendered.contains("SqlValue ::"),
                "`{scalar}` ({arity:?}) produced no SqlValue: {rendered}"
            );
        }
    }
}
