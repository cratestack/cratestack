//! `BigInt` through the per-model generators: struct, primary-key accessor
//! and `<Model>Where`, plus the guard that stops a built-in scalar being
//! silently absent from the filter set (ADR 0019, PR B, risk 3). The SQLite
//! decoder and the descriptor are in `tests_bigint_rows.rs`.

use std::collections::BTreeSet;

use cratestack_core::{Schema, TypeArity};

use super::find_many_where::{generate_where_struct, is_filterable_scalar};
use super::{generate_model_struct_only, generate_primary_key_accessor_impl};
use crate::shared::test_support::{builtin_scalars, field, render, with_rust_decimal};
use crate::shared::{model_name_set, query_scalar_parser_tokens};

const FIXTURE: &str = r#"
auth SessionUser {
  accountId BigInt
}

model Account {
  id BigInt @id
  totalE8 BigInt @range(min: 0, max: 1000000)
  limitE8 BigInt?
  revision BigInt @version
  ownerId BigInt @default(auth().accountId)

  @@allow("all", auth() != null)
}
"#;

pub(super) const BIGINT: &str = ":: cratestack :: BigInt";

pub(super) fn schema() -> Schema {
    cratestack_parser::parse_schema(FIXTURE).expect("the BigInt fixture must parse")
}

#[test]
fn the_model_struct_holds_bigint_fields_and_keeps_default() {
    let schema = schema();
    let model = &schema.models[0];
    let names = model_name_set(&schema.models);
    let rendered = render(&generate_model_struct_only(model, &names, &BTreeSet::new()));
    for expected in [
        format!("pub id : {BIGINT} ,"),
        format!("pub totalE8 : {BIGINT} ,"),
        format!("pub limitE8 : Option < {BIGINT} > ,"),
        format!("pub revision : {BIGINT} ,"),
    ] {
        assert!(
            rendered.contains(&expected),
            "missing `{expected}` in {rendered}"
        );
    }
    // Partial-row decode fills unselected columns with `Default`.
    assert!(rendered.contains("Default"), "{rendered}");
}

#[test]
fn the_primary_key_accessor_is_typed_by_the_newtype() {
    let schema = schema();
    let rendered = render(&generate_primary_key_accessor_impl(&schema.models[0]));
    assert!(
        rendered.contains(&format!("ModelPrimaryKey < {BIGINT} >")),
        "{rendered}"
    );
    assert!(
        rendered.contains(&format!("fn primary_key (& self) -> {BIGINT}")),
        "{rendered}"
    );
}

#[test]
fn the_where_struct_carries_every_bigint_field_with_ordering_operators() {
    let schema = schema();
    let model = &schema.models[0];
    let names = model_name_set(&schema.models);
    let rendered = render(&generate_where_struct(model, &names, &BTreeSet::new()));
    for field in ["id", "totalE8", "limitE8", "revision"] {
        assert!(
            rendered.contains(&format!(
                "pub {field} : Option < :: cratestack :: FieldFilterInput < {BIGINT} >> ,"
            )),
            "`{field}` is missing from <Model>Where, so a client's filter key on it would be \
             ignored and the list returned unfiltered: {rendered}"
        );
    }
    for op in ["gt", "gte", "lt", "lte", "in_", "eq", "ne"] {
        assert!(
            rendered.contains(&format!(". {op} (")),
            "no `{op}` push: {rendered}"
        );
    }
    assert!(
        rendered.contains("is_null"),
        "the optional BigInt field must offer isNull: {rendered}"
    );
}

#[test]
fn filterable_scalars_are_exactly_the_query_parsable_ones() {
    // Guard: `is_filterable_scalar` and `query_scalar_parser_tokens` are two
    // hand-kept lists that must agree. A scalar in the second but not the
    // first is silently absent from `<Model>Where` (the client's key is
    // dropped and the list is over-broad); the reverse generates `.eq()`
    // calls whose value was never proven to parse.
    let enums = BTreeSet::new();
    with_rust_decimal(|| {
        for scalar in builtin_scalars() {
            let f = field("f", scalar, TypeArity::Required);
            let parsable =
                query_scalar_parser_tokens(&f.ty, quote::quote!(value), "f", &enums).is_some();
            assert_eq!(
                is_filterable_scalar(&f, &enums),
                parsable,
                "`{scalar}`: <Model>Where and the query-string parser disagree"
            );
        }
    });
}
