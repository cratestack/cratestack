//! `<Model>ComputedParams` — the typed Rust client's `?computedParams=` /
//! RPC `computedParams` surface (`docs/design/computed-fields.md`'s
//! "Downstream" section: the generated Rust client previously had none,
//! forcing callers to hand-encode a raw JSON string). Emitted once per
//! model that declares at least one *parameterized* `@computed(params:
//! <Type>?)` field, directly alongside that model's `<Model>Client` in
//! the generated `client` module — both the REST (`client::rest::model`)
//! and RPC (`client::rpc::model`) per-model generators call
//! [`generate_model_computed_params_struct`], and both composers
//! (`include_client_schema!` and the server's embedded self-client) share
//! this one call site (`crate::client::generate_client_module`), so the
//! gating predicate (schema-derived, not composer-derived) produces the
//! same surface everywhere automatically.
//!
//! A model with only *bare* `@computed` fields (no `params:`) — or none
//! at all — gets no struct and no extra `get`/`list` parameter: there is
//! nothing typed to send, and the server 422s any `computedParams` key
//! naming a field with no params type anyway
//! (`cratestack-macros/src/axum/model/computed.rs`'s
//! `build_parse_computed_params_fn`).

use cratestack_core::{Field, Model, computed_params_type_name};
use quote::quote;

use crate::builder::{BuilderField, generate_builder};
use crate::shared::ident;

/// A model's `@computed(params: <Type>?)` fields, declaration order.
/// Strictly narrower than `crate::shared::computed_model_fields` (which
/// also includes bare `@computed` fields) — a bare computed field has no
/// params type, so it has no representation on the `computedParams` wire
/// surface at all. This is the gating predicate for "does this model get
/// a `<Model>ComputedParams` struct / a typed `get`/`list` parameter".
pub(super) fn parameterized_computed_fields(model: &Model) -> Vec<&Field> {
    model
        .fields
        .iter()
        .filter(|field| computed_params_type_name(field).is_some())
        .collect()
}

/// `<Model>ComputedParams` struct ident. Only meaningful (i.e. only
/// actually emitted) when [`parameterized_computed_fields`] is non-empty
/// for this model — callers must check that first.
pub(super) fn computed_params_struct_ident(model_name: &str) -> syn::Ident {
    ident(&format!("{model_name}ComputedParams"))
}

/// `Some(ident)` when `model` has at least one parameterized computed
/// field (the struct [`generate_model_computed_params_struct`] emits
/// exists and `get`/`list` should grow the typed parameter), `None`
/// otherwise. The single gating check both the REST and RPC per-model
/// generators call before deciding whether to touch their `get`/`list`
/// token shape at all — an ungated model must emit BIT-IDENTICAL tokens
/// to before this feature existed.
pub(super) fn model_computed_params_ident(model: &Model) -> Option<syn::Ident> {
    if parameterized_computed_fields(model).is_empty() {
        None
    } else {
        Some(computed_params_struct_ident(&model.name))
    }
}

/// Emits `<Model>ComputedParams` + its `to_query_value` helper for a
/// model with at least one parameterized computed field; `None` for a
/// model with none (mirrors [`model_computed_params_ident`]'s gate).
pub(super) fn generate_model_computed_params_struct(
    model: &Model,
) -> Option<proc_macro2::TokenStream> {
    let fields = parameterized_computed_fields(model);
    if fields.is_empty() {
        return None;
    }

    let struct_ident = computed_params_struct_ident(&model.name);
    let field_idents: Vec<syn::Ident> = fields.iter().map(|field| ident(&field.name)).collect();
    let params_type_idents: Vec<syn::Ident> = fields
        .iter()
        .map(|field| {
            let type_name = computed_params_type_name(field)
                .expect("field was filtered to have a params type name above");
            ident(type_name)
        })
        .collect();

    // Every field is `Option<super::types::<P>>`, so no field is
    // required and the emitted builder is non-generic — the same shape
    // `{Model}Where` gets (`crate::model::find_many_where`). This is the
    // one generated object that shipped without the builder every other
    // generated object has had since #656.
    let builder_fields: Vec<BuilderField> = field_idents
        .iter()
        .zip(params_type_idents.iter())
        .map(|(field_ident, params_type_ident)| {
            BuilderField::new(
                field_ident.clone(),
                quote! { ::core::option::Option<super::types::#params_type_ident> },
                false,
            )
        })
        .collect();
    let builder = generate_builder(&struct_ident, &builder_fields);

    Some(quote! {
        /// Typed `?computedParams=` (REST) / RPC `computedParams` payload
        /// for this model's parameterized `@computed` fields — one
        /// optional field per resolver, keyed by the computed field's own
        /// schema name (the wire key `parse_<model>_computed_params`
        /// validates against server-side). `..Default::default()` lets a
        /// caller override just one field.
        #[derive(Debug, Clone, Default, ::cratestack::serde::Serialize)]
        pub struct #struct_ident {
            #(
                #[serde(skip_serializing_if = "::core::option::Option::is_none")]
                pub #field_idents: ::core::option::Option<super::types::#params_type_idents>,
            )*
        }

        impl #struct_ident {
            /// JSON-object text for `?computedParams=` / the RPC frame's
            /// `computedParams` field. `None` when every field is unset,
            /// matching the server's "absent key -> resolver gets None"
            /// default (`docs/design/computed-fields.md`).
            pub fn to_query_value(&self) -> ::core::option::Option<::std::string::String> {
                if [#(self.#field_idents.is_none()),*]
                    .into_iter()
                    .all(|unset| unset)
                {
                    return None;
                }
                Some(
                    ::cratestack::serde_json::to_string(self)
                        .expect("ComputedParams struct fields are all JSON-serializable"),
                )
            }
        }

        #builder
    })
}

#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod tests;
