//! Free-fn filter helpers (`<field>_eq(...)` etc.) emitted alongside
//! each scalar field in a relation order/filter module. Delegates to
//! the per-arity helpers in
//! [`super::filter_builders::functions`](super::filter_builders).

use cratestack_core::{Field, Model};

use crate::shared::{model_name_set, rust_type_tokens, scalar_model_fields, to_snake_case};

use super::filter_builders;
use super::types::RelationPathSegment;

pub(super) fn generate_relation_filter_functions(
    model: &Model,
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let model_names = model_name_set(models);
    scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_scalar_relation_filter_functions(field, wrappers))
        .collect::<Result<Vec<_>, String>>()
        .map(|groups| groups.into_iter().flatten().collect())
}

fn generate_scalar_relation_filter_functions(
    field: &Field,
    wrappers: &[RelationPathSegment],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let field_type = rust_type_tokens(&field.ty);
    let column = to_snake_case(&field.name);
    let mut fns = Vec::new();

    filter_builders::append_required_filter_functions(
        &mut fns, field, wrappers, &field_type, &column,
    );
    filter_builders::append_boolean_filter_functions(
        &mut fns, field, wrappers, &field_type, &column,
    );
    filter_builders::append_required_text_filter_functions(
        &mut fns, field, wrappers, &field_type, &column,
    );
    filter_builders::append_optional_filter_functions(
        &mut fns, field, wrappers, &field_type, &column,
    );
    filter_builders::append_optional_string_filter_functions(
        &mut fns, field, wrappers, &field_type, &column,
    );

    Ok(fns)
}
