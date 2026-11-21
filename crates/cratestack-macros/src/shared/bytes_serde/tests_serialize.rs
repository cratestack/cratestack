//! cratestack#1425: the outbound half of the table in [`super`] — which
//! fields get `serialize_with`, and which projection leaves get wrapped.

use cratestack_core::{SourceSpan, TypeArity, TypeRef};

use super::*;

fn type_ref(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        },
        arity,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    }
}

#[test]
fn byte_valued_family_serializes_with_the_shared_helper_and_uuid_does_not() {
    // cratestack#1425: Bytes/Geography/Geometry write a byte string under
    // CBOR via one generic helper; Uuid's own `Serialize` already does.
    for name in ["Bytes", "Geography", "Geometry"] {
        for arity in [TypeArity::Required, TypeArity::Optional, TypeArity::List] {
            for wrap_for_patch in [false, true] {
                let bytes = bytes_serde(&type_ref(name, arity), wrap_for_patch).unwrap();
                assert_eq!(
                    bytes.serialize_with.expect("serialize_with").to_string(),
                    r#"serialize_with = "::cratestack::serialize_bytes_shape""#,
                    "{name} {arity:?} patch={wrap_for_patch}"
                );
            }
        }
    }
    for arity in [TypeArity::Required, TypeArity::Optional, TypeArity::List] {
        let uuid = bytes_serde(&type_ref("Uuid", arity), false).unwrap();
        assert!(uuid.serialize_with.is_none());
        assert!(
            !bytes_serde_attr(&type_ref("Uuid", arity), false)
                .to_string()
                .contains("serialize_bytes_shape")
        );
    }
}

#[test]
fn leaf_is_wrapped_for_the_bytes_family_only() {
    let expr = quote::quote! { record.field.clone() };
    for name in ["Bytes", "Geography", "Geometry"] {
        let leaf = leaf::projected_leaf_tokens(&type_ref(name, TypeArity::Optional), expr.clone());
        assert!(leaf.to_string().contains("WireBytes"), "{name}");
    }
    for name in ["Uuid", "String", "Json"] {
        let leaf = leaf::projected_leaf_tokens(&type_ref(name, TypeArity::Required), expr.clone());
        assert!(!leaf.to_string().contains("WireBytes"), "{name}");
    }
}
