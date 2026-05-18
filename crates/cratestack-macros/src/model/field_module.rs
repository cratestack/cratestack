//! Per-model `pub mod <model_snake> { ... }` field accessor module:
//! `FieldRef<...>` helpers for every scalar, relation path entry
//! methods, plus the [`selection_module`](super::selection_module)
//! sub-module.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::relation::generate_relation_order_module;
use crate::shared::{
    ident, relation_model_fields, rust_type_tokens, scalar_model_fields, to_snake_case,
};

use super::selection_module::generate_selection_module;

/// Which schema-emission kind is asking for a field module. Server &
/// embedded both emit `*_MODEL` descriptors so they can carry the full
/// surface; client-side does not, so emissions that hard-reference those
/// descriptors must be suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldModuleKind {
    /// Reachable from `include_server_schema!` / `include_embedded_schema!`.
    /// `*_MODEL` descriptors are in scope.
    Server,
    /// Reachable from `include_client_schema!`. `*_MODEL` descriptors are
    /// NOT emitted by the client composer, so any code that references
    /// them (today: `Path::as_include()`) must be skipped — otherwise the
    /// macro output fails to compile.
    Client,
}

pub(crate) fn generate_field_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    generate_field_module_with_kind(model, model_names, models, FieldModuleKind::Server)
}

/// Variant of [`generate_field_module`] that suppresses emissions which
/// reference `*_MODEL` descriptors. The client schema doesn't emit
/// those descriptors, so anything that hard-references them (today:
/// `as_include()` on to-one relation `Path` — see PR #47) has to be
/// skipped.
pub(crate) fn generate_client_field_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    generate_field_module_with_kind(model, model_names, models, FieldModuleKind::Client)
}

fn generate_field_module_with_kind(
    model: &Model,
    model_names: &BTreeSet<&str>,
    models: &[Model],
    kind: FieldModuleKind,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let field_fns = scalar_model_fields(model, model_names).into_iter().map(|field| {
        let function_ident = ident(&field.name);
        let field_type = rust_type_tokens(&field.ty);
        let column = to_snake_case(&field.name);
        quote! {
            #[allow(non_snake_case)]
            pub fn #function_ident() -> ::cratestack::FieldRef<super::models::#model_ident, #field_type> {
                ::cratestack::FieldRef::new(#column)
            }
        }
    });
    let relation_root_fns = relation_model_fields(model, model_names)
        .into_iter()
        .map(|field| {
            let function_ident = ident(&field.name);
            let module_ident = ident(&field.name);
            quote! {
                #[allow(non_snake_case)]
                pub fn #function_ident() -> self::#module_ident::Path {
                    self::#module_ident::Path
                }
            }
        });
    let relation_modules = relation_model_fields(model, model_names)
        .into_iter()
        .map(|relation_field| generate_relation_order_module(model, relation_field, models, kind))
        .collect::<Result<Vec<_>, String>>()?;
    let selection_module = generate_selection_module(model, model_names, models)?;

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            #(#field_fns)*
            #(#relation_root_fns)*

            pub fn select() -> self::selection::Selection {
                self::selection::Selection::default()
            }

            pub fn include_selection() -> self::selection::IncludeSelection {
                self::selection::IncludeSelection::default()
            }

            #(#relation_modules)*

            #selection_module
        }
    })
}
