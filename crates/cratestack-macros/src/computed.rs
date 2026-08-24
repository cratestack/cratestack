//! `ComputedFieldDescriptor` metadata rows and `ComputedFieldResolver`
//! trait methods for every `@computed` field declared on a `type` or a
//! `model` — the generated `computed` module described in
//! `docs/design/computed-fields.md`. Spans both declaration kinds (unlike
//! `crate::types`, which is `type`/`enum`-only), so it lives at the crate
//! root rather than nested under either `types.rs` or `model/`.
//!
//! Replaces the pre-existing `@custom` attribute's `CustomFieldResolver`
//! generation (`type`-only, and nothing ever invoked the generated
//! trait's methods) — `@computed` fields are resolved at response-
//! composition time by a later stage; this module only emits the
//! metadata and the trait shape implementors fill in.

use cratestack_core::{Field, Model, TypeDecl, computed_params_type_name};
use quote::quote;

use crate::shared::{
    computed_model_fields, computed_type_fields, ident, rust_type_tokens_with_scope, schema_lit,
    to_snake_case,
};

fn resolver_method_name(owner_name: &str, field_name: &str) -> String {
    format!(
        "resolve_{}_{}",
        to_snake_case(owner_name),
        to_snake_case(field_name)
    )
}

fn computed_field_descriptors(
    owner_name: &str,
    fields: &[&Field],
) -> Vec<proc_macro2::TokenStream> {
    fields
        .iter()
        .map(|field| {
            let owner = schema_lit(owner_name);
            let field_name = schema_lit(&field.name);
            let resolver_method = schema_lit(&resolver_method_name(owner_name, &field.name));
            let params_type = match computed_params_type_name(field) {
                Some(name) => {
                    let name = schema_lit(name);
                    quote! { Some(#name) }
                }
                None => quote! { None },
            };
            quote! {
                ComputedFieldDescriptor {
                    owner: #owner,
                    field: #field_name,
                    resolver_method: #resolver_method,
                    params_type: #params_type,
                }
            }
        })
        .collect()
}

/// `ComputedFieldDescriptor` rows for a `type` declaration's own
/// `@computed` fields.
pub(crate) fn generate_type_computed_field_descriptors(
    ty: &TypeDecl,
) -> Vec<proc_macro2::TokenStream> {
    computed_field_descriptors(&ty.name, &computed_type_fields(ty))
}

/// `ComputedFieldDescriptor` rows for a `model`'s own `@computed` fields.
pub(crate) fn generate_model_computed_field_descriptors(
    model: &Model,
) -> Vec<proc_macro2::TokenStream> {
    computed_field_descriptors(&model.name, &computed_model_fields(model))
}

/// `source`/`params`/`ctx` are the *server-side* struct/params shapes —
/// `owner_name` is re-exported at `super::#owner_ident` regardless of
/// whether it's a `model` (`pub use models::*`) or a `type`
/// (`pub use types::*`), so both owner kinds resolve through the same
/// `super::` path.
fn computed_field_resolver_methods(
    owner_name: &str,
    fields: &[&Field],
) -> Vec<proc_macro2::TokenStream> {
    let owner_ident = ident(owner_name);
    fields
        .iter()
        .map(|field| {
            let method_ident = ident(&resolver_method_name(owner_name, &field.name));
            let return_type = rust_type_tokens_with_scope(&field.ty, true);
            let params_arg = match computed_params_type_name(field) {
                Some(params_type_name) => {
                    let params_ident = ident(params_type_name);
                    quote! { params: ::core::option::Option<&super::#params_ident>, }
                }
                None => quote! {},
            };

            quote! {
                fn #method_ident(
                    &self,
                    db: &super::Cratestack,
                    source: &super::#owner_ident,
                    #params_arg
                    ctx: &::cratestack::CratestackContext,
                ) -> impl ::core::future::Future<Output = Result<#return_type, ::cratestack::CratestackError>> + Send;
            }
        })
        .collect()
}

/// `ComputedFieldResolver` trait methods for a `type` declaration's own
/// `@computed` fields.
pub(crate) fn generate_type_computed_field_resolver_methods(
    ty: &TypeDecl,
) -> Vec<proc_macro2::TokenStream> {
    computed_field_resolver_methods(&ty.name, &computed_type_fields(ty))
}

/// `ComputedFieldResolver` trait methods for a `model`'s own `@computed`
/// fields.
pub(crate) fn generate_model_computed_field_resolver_methods(
    model: &Model,
) -> Vec<proc_macro2::TokenStream> {
    computed_field_resolver_methods(&model.name, &computed_model_fields(model))
}
