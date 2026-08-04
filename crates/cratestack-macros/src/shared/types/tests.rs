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
    }
}

#[test]
fn required_vector_field_generates_vec_f32() {
    let ty = vector_type_ref(TypeArity::Required, 1536);
    let tokens = rust_type_tokens(&ty);
    assert_eq!(tokens.to_string(), quote! { Vec < f32 > }.to_string());
}

#[test]
fn optional_vector_field_wraps_in_option() {
    let ty = vector_type_ref(TypeArity::Optional, 3);
    let tokens = rust_type_tokens(&ty);
    assert_eq!(
        tokens.to_string(),
        quote! { Option < Vec < f32 > > }.to_string()
    );
}
