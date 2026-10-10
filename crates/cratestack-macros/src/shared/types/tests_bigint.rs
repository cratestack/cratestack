//! `BigInt` in the Rust-type and query-string token generators, and the
//! guards that keep a built-in scalar from falling through their catch-alls
//! (ADR 0019, PR B).

use std::collections::BTreeSet;

use cratestack_core::TypeArity;

use super::*;
use crate::shared::test_support::{builtin_scalars, render, type_ref, with_rust_decimal};
use crate::shared::{rust_type_tokens_with_wire_scope, supports_comparison};

const BIGINT: &str = ":: cratestack :: BigInt";

#[test]
fn bigint_is_the_cratestack_newtype_in_every_arity() {
    for (arity, expected) in [
        (TypeArity::Required, BIGINT.to_owned()),
        (TypeArity::Optional, format!("Option < {BIGINT} >")),
        (TypeArity::List, format!("Vec < {BIGINT} >")),
    ] {
        let ty = type_ref("BigInt", arity);
        assert_eq!(render(&rust_type_tokens(&ty)), expected, "{arity:?}");
        assert_eq!(
            render(&rust_type_tokens_with_wire_scope(&ty, &BTreeSet::new())),
            expected,
            "the wire-scope mapping must not drift from the model mapping ({arity:?})"
        );
    }
}

#[test]
fn bigint_is_not_a_struct_in_the_schema_module() {
    // The catch-all arm of `rust_type_tokens` turns an unknown name into
    // `super::<Name>`, a path that does not exist for `BigInt`.
    let rendered = render(&rust_type_tokens(&type_ref("BigInt", TypeArity::Required)));
    assert!(!rendered.contains("super"), "{rendered}");
}

#[test]
fn no_builtin_scalar_resolves_to_a_user_declared_type() {
    // Guard: `super::<Name>` is what both type mappings emit for a name they
    // do not know, which for a built-in scalar is a compile error in the
    // consumer's crate, not here. Every scalar the parser accepts must be
    // named in both match tables.
    let no_bearing = BTreeSet::new();
    with_rust_decimal(|| {
        for scalar in builtin_scalars() {
            for arity in [TypeArity::Required, TypeArity::Optional] {
                let ty = type_ref(scalar, arity);
                for (which, rendered) in [
                    ("rust_type_tokens", render(&rust_type_tokens(&ty))),
                    (
                        "rust_type_tokens_with_wire_scope",
                        render(&rust_type_tokens_with_wire_scope(&ty, &no_bearing)),
                    ),
                ] {
                    assert!(
                        !rendered.contains("super ::"),
                        "{which}({scalar}, {arity:?}) fell through to a user-declared type: {rendered}"
                    );
                }
            }
        }
    });
}

#[test]
fn query_string_filters_parse_bigint_with_the_canonical_grammar() {
    let ty = type_ref("BigInt", TypeArity::Required);
    let tokens = query_scalar_parser_tokens(&ty, quote!(value), "totalE8", &BTreeSet::new())
        .expect("a BigInt field must be query-filterable");
    let rendered = render(&tokens);
    assert!(
        rendered.contains(&format!("parse :: < {BIGINT} >")),
        "must go through `FromStr for BigInt`, not `i64::from_str`: {rendered}"
    );
    assert!(
        !rendered.contains("parse :: < i64 >"),
        "i64::from_str accepts `+5` and `007`: {rendered}"
    );
    assert!(rendered.contains("\"totalE8\""), "{rendered}");
    assert!(rendered.contains("BadRequest"), "{rendered}");
}

#[test]
fn query_string_in_list_parses_each_element_as_bigint() {
    let ty = type_ref("BigInt", TypeArity::Required);
    let tokens = query_scalar_list_parser_tokens(&ty, "totalE8", &BTreeSet::new())
        .expect("a BigInt field must support `__in`");
    let rendered = render(&tokens);
    assert!(
        rendered.contains(&format!("parse :: < {BIGINT} >")),
        "{rendered}"
    );
    assert!(
        rendered.contains("__in requires at least one value"),
        "{rendered}"
    );
    assert!(rendered.contains("\"totalE8\""), "{rendered}");
}

#[test]
fn bigint_supports_the_comparison_operators() {
    let mut field = crate::shared::test_support::field("totalE8", "BigInt", TypeArity::Required);
    assert!(supports_comparison(&field));
    field.ty.arity = TypeArity::Optional;
    assert!(
        !supports_comparison(&field),
        "optional fields stay equality-only on the untyped route, as `Int` does"
    );
}

/// Scalars the untyped `?where=` route deliberately cannot filter on. Anything
/// else the parser lists must have a query parser, or its filter key is
/// dropped without an error and a list comes back unfiltered.
const NOT_QUERY_FILTERABLE: [&str; 5] = ["Json", "Bytes", "Vector", "Geography", "Geometry"];

#[test]
fn every_builtin_scalar_is_query_filterable_or_named_as_not() {
    let enums = BTreeSet::new();
    with_rust_decimal(|| {
        for scalar in builtin_scalars() {
            let parser = query_scalar_parser_tokens(
                &type_ref(scalar, TypeArity::Required),
                quote!(value),
                "f",
                &enums,
            );
            let listed = NOT_QUERY_FILTERABLE.contains(&scalar);
            assert_eq!(
                parser.is_none(),
                listed,
                "`{scalar}`: query_scalar_parser_tokens is {} but the not-filterable list says {listed}; \
                 add a parser arm, or name the scalar in NOT_QUERY_FILTERABLE on purpose",
                if parser.is_none() { "None" } else { "Some" },
            );
        }
    });
}

#[test]
fn bigint_field_definition_is_a_plain_pub_field() {
    // No serde attribute: the newtype's own `Serialize`/`Deserialize` carry
    // the string form, which is the point of D3 (a `with` attribute would be
    // skipped by `?fields=` projection and the generic `FieldFilterInput`).
    let field = crate::shared::test_support::field("totalE8", "BigInt", TypeArity::Required);
    let rendered = render(&field_definition(&field, false, true));
    assert_eq!(rendered, format!("pub totalE8 : {BIGINT} ,"));
    let patched = render(&field_definition(&field, true, true));
    assert_eq!(patched, format!("pub totalE8 : Option < {BIGINT} > ,"));
}
