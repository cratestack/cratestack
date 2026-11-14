//! `Uuid` joins the `bytes_serde` choke point (cratestack#1359): same
//! `(wrap_for_patch, arity)` table, `uuid` helper family. As in `tests`,
//! the load-bearing check is that each helper's return type matches the
//! field type `field_type` emits — a mismatch is a compile error in
//! generated code that no unit test here could otherwise catch.

use cratestack_core::{Field, SourceSpan};

use super::*;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn uuid_ref(arity: TypeArity) -> TypeRef {
    TypeRef {
        name: "Uuid".to_owned(),
        name_span: span(),
        arity,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    }
}

#[test]
fn uuid_helpers_match_the_emitted_field_types_and_default_flag() {
    let expectations = [
        (TypeArity::Required, false, "deserialize_uuid", false),
        (TypeArity::Optional, false, "deserialize_optional_uuid", true),
        (TypeArity::List, false, "deserialize_uuid_list", false),
        (TypeArity::Required, true, "deserialize_optional_uuid", true),
        (TypeArity::Optional, true, "deserialize_double_option_uuid", true),
        (TypeArity::List, true, "deserialize_optional_uuid_list", true),
    ];
    for (arity, patch, helper, needs_default) in expectations {
        let serde = bytes_serde(&uuid_ref(arity), patch).expect("Uuid opts in");
        assert_eq!(
            serde.deserialize_with.to_string(),
            format!("deserialize_with = \"::cratestack::{helper}\""),
            "{arity:?} patch={patch}"
        );
        assert_eq!(serde.needs_default, needs_default, "{arity:?} patch={patch}");

        let field = Field {
            name: "id".to_owned(),
            name_span: span(),
            ty: uuid_ref(arity),
            attributes: Vec::new(),
            docs: Vec::new(),
            span: span(),
        };
        let ty = super::super::types::field_type(&field, patch, true).to_string();
        let expected = match (patch, arity) {
            (false, TypeArity::Required) => "U",
            (false, TypeArity::Optional) | (true, TypeArity::Required) => "Option < U >",
            (false, TypeArity::List) => "Vec < U >",
            (true, TypeArity::Optional) => "Option < Option < U > >",
            (true, TypeArity::List) => "Option < Vec < U > >",
        };
        assert_eq!(
            ty,
            expected.replace('U', ":: cratestack :: uuid :: Uuid"),
            "field type for {arity:?} patch={patch}"
        );
    }
}

#[test]
fn uuid_attribute_for_a_type_block_field_carries_default_only_for_options() {
    let required = bytes_serde_attr(&uuid_ref(TypeArity::Required), false).to_string();
    assert!(required.contains("deserialize_uuid") && !required.contains("default"));
    let optional = bytes_serde_attr(&uuid_ref(TypeArity::Optional), false).to_string();
    assert!(optional.contains("default") && optional.contains("deserialize_optional_uuid"));
}
