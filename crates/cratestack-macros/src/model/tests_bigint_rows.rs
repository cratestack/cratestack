//! `BigInt` through the SQLite row decoder and the descriptor (ADR 0019,
//! PR B, risk 3): the embedded decode arms, the guard on the decoder's
//! `row.get(name)?` default, and the auth-derived default. Split from
//! `tests_bigint.rs` per the repo's 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::TypeArity;

use super::row_sqlite::sqlite_row_field_tokens;
use super::tests_bigint::{BIGINT, schema};
use crate::shared::test_support::{builtin_scalars, field, render, with_rust_decimal};

fn sqlite_decode(arity: TypeArity) -> String {
    render(&sqlite_row_field_tokens(
        &field("totalE8", "BigInt", arity),
        &BTreeSet::new(),
    ))
}

#[test]
fn sqlite_decodes_bigint_from_an_i64_without_a_driver_impl_on_the_newtype() {
    assert_eq!(
        sqlite_decode(TypeArity::Required),
        format!("totalE8 : {BIGINT} :: new (row . get :: < _ , i64 > (\"totalE8\") ?) ,")
    );
    let optional = sqlite_decode(TypeArity::Optional);
    assert!(optional.contains("Option < i64 >"), "{optional}");
    assert!(
        optional.contains(&format!(". map ({BIGINT} :: new)")),
        "{optional}"
    );
}

/// Scalars the SQLite decoder hands to `row.get(name)?` and rusqlite's own
/// `FromSql` resolves. Every other built-in scalar needs an explicit arm.
const RUSQLITE_NATIVE: [&str; 8] = [
    "String",
    "Cuid",
    "Int",
    "Float",
    "Bytes",
    "Vector",
    "Geography",
    "Geometry",
];

#[test]
fn every_builtin_scalar_has_a_sqlite_arm_or_is_natively_decodable() {
    with_rust_decimal(|| {
        for scalar in builtin_scalars() {
            let rendered = render(&sqlite_row_field_tokens(
                &field("f", scalar, TypeArity::Required),
                &BTreeSet::new(),
            ));
            let default_arm = rendered == "f : row . get (\"f\") ? ,";
            assert_eq!(
                default_arm,
                RUSQLITE_NATIVE.contains(&scalar),
                "`{scalar}` reaches the `row.get(name)?` default arm without being listed as \
                 natively decodable (or is listed and has an explicit arm): {rendered}"
            );
        }
    });
}

#[test]
fn an_auth_derived_bigint_default_is_the_bigint_kind() {
    let schema = schema();
    let model = &schema.models[0];
    let defaults = super::descriptor::generate_model_descriptor(
        model,
        &schema.models,
        &schema.types,
        &schema.enums,
        schema.auth.as_ref(),
    )
    .expect("a BigInt descriptor must generate");
    let rendered = render(&defaults);
    assert!(
        rendered.contains(":: cratestack :: CreateDefaultType :: BigInt"),
        "{rendered}"
    );
    assert!(
        rendered.contains("auth_field : \"accountId\""),
        "{rendered}"
    );
    assert!(
        rendered.contains(&format!("ModelDescriptor < Account , {BIGINT} >")),
        "the descriptor's key type is the newtype: {rendered}"
    );
    assert!(
        rendered.contains("Some (\"revision\")"),
        "@version column: {rendered}"
    );
}
