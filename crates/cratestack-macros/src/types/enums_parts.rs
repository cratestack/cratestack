//! The per-variant and per-kind pieces of an enum's generated type, shared
//! by the server/embedded and client generators in `enums.rs`.

use cratestack_core::EnumDecl;
use quote::quote;

use super::enums_ordinal::ordinal_impls;
use crate::shared::{doc_attrs, ident, schema_lit};

pub(super) fn variant_tokens(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let variant_ident = ident(&variant.name);
            let variant_docs = doc_attrs(&variant.docs);
            let name = schema_lit(&variant.name);
            // The first variant is the `Default` — needed so model
            // structs with enum fields can `derive(Default)` for the
            // column-projection `Projection<T>` placeholder values.
            let default_attr = if index == 0 {
                quote! { #[default] }
            } else {
                quote! {}
            };
            // An ordinal variant has its discriminant and no serde rename:
            // the hand-written impls encode it as the number.
            match variant.ordinal.and_then(|o| i16::try_from(o).ok()) {
                Some(ordinal) => quote! {
                    #variant_docs
                    #default_attr
                    #variant_ident = #ordinal,
                },
                None => quote! {
                    #variant_docs
                    #[serde(rename = #name)]
                    #default_attr
                    #variant_ident,
                },
            }
        })
        .collect()
}

pub(super) fn as_str_arms(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = ident(&variant.name);
            let name = schema_lit(&variant.name);
            quote! { Self::#variant_ident => #name, }
        })
        .collect()
}

/// Comma-joined variant names, baked in as one string literal — issue
/// #928's rejection message (and, by extension, the query-filter
/// `BadRequest` that wraps this `FromStr` error, in
/// `shared::types::query_scalar_parser_tokens`) needs to name the
/// accepted values, not just report that the given one didn't match.
pub(super) fn accepted_variants_lit(enum_decl: &EnumDecl) -> syn::LitStr {
    let known = enum_decl
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    schema_lit(&known)
}

pub(super) fn parse_arms(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = ident(&variant.name);
            let name = schema_lit(&variant.name);
            quote! { #name => Ok(Self::#variant_ident), }
        })
        .collect()
}

/// A string enum derives its serde impls; an ordinal one is `repr(i16)`
/// and gets hand-written ones (`enums_ordinal::ordinal_impls`).
pub(super) fn derives(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    if enum_decl.is_ordinal() {
        quote! {
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
            #[repr(i16)]
        }
    } else {
        quote! {
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        }
    }
}

pub(super) fn ordinal_impls_if_any(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    if enum_decl.is_ordinal() {
        ordinal_impls(enum_decl)
    } else {
        quote! {}
    }
}

/// What a server or embedded enum binds as: its ordinal, or its name.
pub(super) fn server_sql_value(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    if enum_decl.is_ordinal() {
        quote! { ::cratestack::SqlValue::Int(i64::from(self.ordinal())) }
    } else {
        quote! { ::cratestack::SqlValue::String(self.to_string()) }
    }
}
