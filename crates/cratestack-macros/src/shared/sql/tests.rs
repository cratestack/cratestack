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
