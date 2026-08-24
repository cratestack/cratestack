//! `@computed` field resolution arms spliced into
//! `serialize_<model>_model_value` after the stored-field projection +
//! relation includes (`docs/design/computed-fields.md`'s "Models
//! (Postgres server schemas)" section). Shares field metadata with
//! [`super::super::computed::model_computed_fields`] — the
//! `?computedParams=` key legality validated there and the resolver
//! dispatch here must agree on exactly the same field list, so both walk
//! the same [`super::super::computed::ModelComputedField`] slice.

use quote::quote;

use super::super::computed::ModelComputedField;

/// One `if` block per computed field: skip resolution entirely when
/// `?fields=` was supplied and excludes this field (never call the
/// resolver in that case — the field simply isn't projected).
pub(in super::super) fn build_computed_resolve_arms(
    fields: &[ModelComputedField],
) -> Vec<proc_macro2::TokenStream> {
    fields
        .iter()
        .map(|field| {
            let name = &field.name;
            let resolver_method_ident = &field.resolver_method_ident;
            let resolve_call = match (&field.params_type_ident, &field.params_type_name) {
                (Some(params_type_ident), Some(params_type_name)) => quote! {
                    let params = match computed_params.and_then(|map| map.get(#name)) {
                        Some(raw_value) => Some(
                            ::cratestack::serde_json::from_value::<super::#params_type_ident>(raw_value.clone())
                                .map_err(|error| CratestackError::Validation(format!(
                                    "invalid computedParams for field '{}' (expected {}): {}",
                                    #name,
                                    #params_type_name,
                                    error,
                                )))?
                        ),
                        None => None,
                    };
                    resolvers.#resolver_method_ident(db, record, params.as_ref(), ctx).await?
                },
                _ => quote! {
                    resolvers.#resolver_method_ident(db, record, ctx).await?
                },
            };
            quote! {
                let should_resolve = match &selection.fields {
                    Some(fields) => fields.iter().any(|selected| selected == #name),
                    None => true,
                };
                if should_resolve {
                    let value = { #resolve_call };
                    object.insert(#name.to_owned(), ::cratestack::ProjectedValue::leaf(value));
                }
            }
        })
        .collect()
}
