//! The load-bearing property here isn't which string comes back — it's
//! that the helper each shape names has a return type matching the field
//! type `super::super::types::field_type` emits for the same
//! `(wrap_for_patch, arity)`. A mismatch is a compile error in generated
//! code, which no unit test in this crate can catch, so
//! [`emitted_helper_return_types_match_the_field_types`] pins the two
//! tables against each other in one place and the module doc's table
//! spells the pairing out.

use cratestack_core::SourceSpan;

use super::*;

fn synthetic_span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn type_ref(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: Vec::new(),
        int_args: Vec::new(),
    }
}

/// The exact `#[serde(...)]` attribute the `type`-block and
/// procedure-`Args` emitters splice, with `TokenStream::to_string`'s
/// punctuation spacing (`# [serde (a , b)]`) normalised back to how it
/// reads in source, so the expectations below are legible.
fn rendered(name: &str, arity: TypeArity, wrap_for_patch: bool) -> Option<String> {
    let attribute = bytes_serde_attr(&type_ref(name, arity), wrap_for_patch).to_string();
    (!attribute.is_empty()).then(|| {
        attribute
            .replace("# [serde (", "#[serde(")
            .replace(" , ", ", ")
    })
}

#[test]
fn only_bytes_fields_opt_in() {
    for name in ["String", "Int", "Json", "Vector", "Uuid", "SomeCustomType"] {
        for arity in [TypeArity::Required, TypeArity::Optional, TypeArity::List] {
            for wrap_for_patch in [false, true] {
                assert!(
                    bytes_serde(&type_ref(name, arity), wrap_for_patch).is_none(),
                    "{name} at {arity:?} (patch={wrap_for_patch}) must not get a deserialize_with"
                );
            }
        }
    }
}

#[test]
fn unwrapped_shapes_pick_the_matching_helper() {
    assert_eq!(
        rendered("Bytes", TypeArity::Required, false).as_deref(),
        Some(r#"#[serde(deserialize_with = "::cratestack::deserialize_bytes")]"#)
    );
    assert_eq!(
        rendered("Bytes", TypeArity::List, false).as_deref(),
        Some(r#"#[serde(deserialize_with = "::cratestack::deserialize_bytes_list")]"#)
    );
}

#[test]
fn patch_wrapped_required_and_unwrapped_optional_share_a_helper() {
    // Both generate `Option<Vec<u8>>` — the `Option` comes from arity in
    // one case and from patch-wrapping in the other, but the field type,
    // and therefore the helper, is identical.
    let unwrapped_optional = rendered("Bytes", TypeArity::Optional, false);
    let patched_required = rendered("Bytes", TypeArity::Required, true);
    assert_eq!(unwrapped_optional, patched_required);
    assert_eq!(
        unwrapped_optional.as_deref(),
        Some(r#"#[serde(default, deserialize_with = "::cratestack::deserialize_optional_bytes")]"#)
    );
}

#[test]
fn patch_wrapped_nullable_uses_the_double_option_variant() {
    // `Option<Option<Vec<u8>>>`. Reusing the generic
    // `deserialize_double_option` here would silently reinstate the bug:
    // its `T: Deserialize` bound resolves to `Vec<u8>`'s strict blanket
    // impl (cratestack#783).
    assert_eq!(
        rendered("Bytes", TypeArity::Optional, true).as_deref(),
        Some(
            r#"#[serde(default, deserialize_with = "::cratestack::deserialize_double_option_bytes")]"#
        )
    );
}

#[test]
fn patch_wrapped_list_uses_the_optional_list_helper() {
    assert_eq!(
        rendered("Bytes", TypeArity::List, true).as_deref(),
        Some(
            r#"#[serde(default, deserialize_with = "::cratestack::deserialize_optional_bytes_list")]"#
        )
    );
}

#[test]
fn default_rides_along_exactly_when_the_field_type_is_an_option() {
    // Without `default` an omitted nullable `Bytes` field would become a
    // hard decode error, because a custom `deserialize_with` opts out of
    // serde-derive's implicit missing-`Option`-is-`None` handling. With a
    // spurious `default` on a non-`Option` field, a missing required
    // field would silently decode as empty bytes. Both are regressions,
    // so the flag has to track the `Option`-ness of the emitted type
    // exactly.
    let expectations = [
        (TypeArity::Required, false, false),
        (TypeArity::Optional, false, true),
        (TypeArity::List, false, false),
        (TypeArity::Required, true, true),
        (TypeArity::Optional, true, true),
        (TypeArity::List, true, true),
    ];
    for (arity, wrap_for_patch, expected) in expectations {
        let bytes =
            bytes_serde(&type_ref("Bytes", arity), wrap_for_patch).expect("Bytes always opts in");
        assert_eq!(
            bytes.needs_default, expected,
            "{arity:?} (patch={wrap_for_patch}) default expectation"
        );
    }
}

#[test]
fn emitted_helper_return_types_match_the_field_types() {
    // The cross-check the module doc's table describes: for every shape,
    // the type `field_type` puts on the field and the type the chosen
    // helper returns must be spelled identically. `bytes_deserialize_with`
    // is what the model/CRUD emitter splices, so it's exercised here too.
    let expectations = [
        (
            TypeArity::Required,
            false,
            "Vec < u8 >",
            "deserialize_bytes",
        ),
        (
            TypeArity::Optional,
            false,
            "Option < Vec < u8 > >",
            "deserialize_optional_bytes",
        ),
        (
            TypeArity::List,
            false,
            "Vec < Vec < u8 > >",
            "deserialize_bytes_list",
        ),
        (
            TypeArity::Required,
            true,
            "Option < Vec < u8 > >",
            "deserialize_optional_bytes",
        ),
        (
            TypeArity::Optional,
            true,
            "Option < Option < Vec < u8 > > >",
            "deserialize_double_option_bytes",
        ),
        (
            TypeArity::List,
            true,
            "Option < Vec < Vec < u8 > > >",
            "deserialize_optional_bytes_list",
        ),
    ];

    for (arity, wrap_for_patch, expected_type, expected_helper) in expectations {
        let field = cratestack_core::Field {
            ty: type_ref("Bytes", arity),
            ..bytes_field(arity)
        };
        assert_eq!(
            super::super::types::field_type(&field, wrap_for_patch, true).to_string(),
            expected_type,
            "field type for {arity:?} (patch={wrap_for_patch})"
        );
        let argument = bytes_deserialize_with(&field.ty, wrap_for_patch)
            .expect("Bytes always opts in")
            .to_string();
        assert!(
            argument.contains(expected_helper),
            "{arity:?} (patch={wrap_for_patch}) picked {argument}, expected {expected_helper}"
        );
    }
}

fn bytes_field(arity: TypeArity) -> cratestack_core::Field {
    cratestack_core::Field {
        name: "payload".to_owned(),
        name_span: synthetic_span(),
        ty: type_ref("Bytes", arity),
        attributes: Vec::new(),
        docs: Vec::new(),
        span: synthetic_span(),
    }
}
