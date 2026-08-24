//! Shared `@computed` field metadata for a model, plus the
//! `parse_<model>_computed_params` generator. Consumed by both the
//! `?computedParams=` validator generated here and the response-
//! composition resolve arms in
//! [`super::serializers::computed_fields`] — the two must agree on
//! exactly which field names are legal `computedParams` keys, so both
//! walk the same [`ModelComputedField`] list.
//!
//! Validation happens once, before any DB access (called from the GET/
//! LIST dispatch fns right after selection validation) — see
//! `docs/design/computed-fields.md`'s "Parameterized resolvers on the
//! wire" section for the exact error taxonomy this enforces.

use cratestack_core::{Model, computed_params_type_name};
use quote::quote;

use crate::shared::{computed_model_fields, ident, to_snake_case};

use super::prep::ModelHandlerPrep;

/// One `@computed` field's codegen-relevant shape, scoped to a single
/// model (owner is implicit — always that model).
pub(super) struct ModelComputedField {
    pub(super) name: String,
    pub(super) resolver_method_ident: syn::Ident,
    pub(super) params_type_ident: Option<syn::Ident>,
    /// Kept alongside `params_type_ident` (rather than derived from it via
    /// `to_string`) so a params type whose name needed raw-identifier
    /// escaping (`ident()`'s `r#...` path) still reports its real,
    /// unescaped schema name in generated error messages.
    pub(super) params_type_name: Option<String>,
}

pub(super) fn model_computed_fields(model: &Model) -> Vec<ModelComputedField> {
    computed_model_fields(model)
        .into_iter()
        .map(|field| {
            let resolver_method_ident = ident(&format!(
                "resolve_{}_{}",
                to_snake_case(&model.name),
                to_snake_case(&field.name)
            ));
            let params_type_name = computed_params_type_name(field).map(str::to_owned);
            let params_type_ident = params_type_name.as_deref().map(ident);
            ModelComputedField {
                name: field.name.clone(),
                resolver_method_ident,
                params_type_ident,
                params_type_name,
            }
        })
        .collect()
}

/// `parse_<model>_computed_params` — validates a raw `?computedParams=`
/// value's *keys* against this model's parameterized computed fields (and
/// against the request's `?fields=` selection). Each key's value is left
/// as an un-decoded `serde_json::Value`: typed deserialization happens at
/// resolve time in `serializers::computed_fields`, where the field's
/// params type is known.
pub(super) fn build_parse_computed_params_fn(
    p: &ModelHandlerPrep,
    fields: &[ModelComputedField],
) -> proc_macro2::TokenStream {
    let parse_computed_params_ident = &p.parse_computed_params_ident;
    let model_name = &p.model_name;

    let parameterized: Vec<&ModelComputedField> = fields
        .iter()
        .filter(|field| field.params_type_ident.is_some())
        .collect();

    // Cheap fast path (docs/design/computed-fields.md): a model with no
    // parameterized computed field at all can never accept a
    // `computedParams` value, so reject any supplied one immediately with
    // one clear message instead of parsing JSON only to fall through
    // every match arm to the same generic "not a legal key" error.
    if parameterized.is_empty() {
        return quote! {
            fn #parse_computed_params_ident(
                raw: Option<&str>,
                _selection: &ModelSelectionQuery,
            ) -> Result<ComputedParamsQuery, CratestackError> {
                match raw {
                    None => Ok(ComputedParamsQuery::new()),
                    Some(_) => Err(CratestackError::Validation(format!(
                        "model {} has no parameterized computed fields",
                        #model_name,
                    ))),
                }
            }
        };
    }

    let legal_arms = parameterized.iter().map(|field| {
        let name = &field.name;
        quote! {
            #name => {
                if let Some(fields) = &selection.fields {
                    if !fields.iter().any(|selected| selected == #name) {
                        return Err(CratestackError::Validation(format!(
                            "computedParams key '{}' is excluded by ?fields= for model {}",
                            #name,
                            #model_name,
                        )));
                    }
                }
            }
        }
    });

    quote! {
        fn #parse_computed_params_ident(
            raw: Option<&str>,
            selection: &ModelSelectionQuery,
        ) -> Result<ComputedParamsQuery, CratestackError> {
            let Some(raw) = raw else {
                return Ok(ComputedParamsQuery::new());
            };
            let object = ::cratestack::parse_computed_params_object(raw)?;
            for key in object.keys() {
                match key.as_str() {
                    #(#legal_arms)*
                    other => {
                        return Err(CratestackError::Validation(format!(
                            "computedParams key '{}' does not name a parameterized computed field of model {}",
                            other,
                            #model_name,
                        )));
                    }
                }
            }
            Ok(object)
        }
    }
}
