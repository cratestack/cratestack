use cratestack_core::TypeRef;

use crate::text::span_contains;

pub(crate) fn render_type_ref(ty: &TypeRef) -> String {
    let base = if ty.generic_args.is_empty() {
        ty.name.clone()
    } else {
        format!(
            "{}<{}>",
            ty.name,
            ty.generic_args
                .iter()
                .map(render_type_ref)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    match ty.arity {
        cratestack_core::TypeArity::Required => base,
        cratestack_core::TypeArity::Optional => format!("{base}?"),
        cratestack_core::TypeArity::List => format!("{base}[]"),
    }
}

pub(crate) fn type_ref_at_offset(ty: &TypeRef, offset: usize) -> bool {
    span_contains(ty.name_span, offset)
        || ty
            .generic_args
            .iter()
            .any(|inner| type_ref_at_offset(inner, offset))
}

pub(crate) fn nested_type_reference_name_at_offset(ty: &TypeRef, offset: usize) -> Option<&str> {
    if span_contains(ty.name_span, offset) {
        return Some(ty.name.as_str());
    }
    ty.generic_args
        .iter()
        .find_map(|inner| nested_type_reference_name_at_offset(inner, offset))
}
