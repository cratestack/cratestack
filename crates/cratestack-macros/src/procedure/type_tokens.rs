//! Return/argument type-token resolution for procedures — shared by the
//! server `pub mod <procedure>` and the client module. Split out of
//! `types.rs` per the repo's 200-LoC file convention; `types.rs` keeps
//! the `Args` struct emission (incl. its builder) and calls back into
//! [`procedure_type_tokens`] here for each field's type.

use std::collections::BTreeSet;

use cratestack_core::{TypeArity, TypeDecl, TypeRef};
use quote::quote;

use crate::shared::ident;

pub(super) fn procedure_output_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    procedure_type_tokens(type_ref, types, enum_names)
}

pub(crate) fn procedure_client_output_item_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => crate::shared::decimal_backend::current_decimal_type_tokens(),
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let model_ident = ident(other);
            quote! { super::#model_ident }
        }
    }
}

/// The type tokens for one procedure argument or return type. `pub(super)`
/// rather than private: `types.rs`, a sibling submodule of `procedure`,
/// calls this for each `Args` field's type — both to emit the field and,
/// via `procedure_arg_builder_fields`, to feed the same tokens into that
/// field's builder setter.
pub(super) fn procedure_type_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = procedure_type_tokens(item, types, enum_names);
        return quote! { ::cratestack::Page<#item_type> };
    }

    if type_ref.is_find_many() {
        let item = type_ref
            .find_many_item()
            .expect("validated FindMany<T> should include an item type");
        // Unlike `Page<T>`, `FindMany<T>`'s item is always a declared
        // model (parser-enforced), never a builtin scalar — so this maps
        // straight to that model's own generated `<Model>FindManyInput`
        // (`crates/cratestack-macros/src/model/find_many_input.rs`)
        // rather than recursing through the generic scalar/model
        // resolution `procedure_item_type_tokens` does for `Page<T>`.
        let find_many_ident = ident(&format!("{}FindManyInput", item.name));
        return quote! { super::super::#find_many_ident };
    }

    let inner = procedure_item_type_tokens(type_ref, types, enum_names);

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

/// Scalar/model mapping for one element of `type_ref`, ignoring arity and
/// the `Page<T>` wrapper entirely — i.e. what a `Vec<T>`'s `T` is. Shared
/// by [`procedure_type_tokens`] (which wraps it per `type_ref.arity`) and
/// [`procedure_stream_item_tokens`] (which never wraps it: a `@stream`
/// procedure's `Stream<Item = Result<T, _>>` wants the element type
/// directly, not `Vec<T>`).
fn procedure_item_type_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => crate::shared::decimal_backend::current_decimal_type_tokens(),
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        "PageInput" => quote! { ::cratestack::PageInput },
        other => {
            let item_ident = ident(other);
            if types.iter().any(|ty| ty.name == other) || enum_names.contains(other) {
                quote! { super::super::types::#item_ident }
            } else {
                quote! { super::super::#item_ident }
            }
        }
    }
}

/// Item type tokens for a `@stream`-marked procedure's stream-shaped
/// `ProcedureRegistry` trait method. Callers must only invoke this once
/// `cratestack-parser` has confirmed `type_ref.arity == TypeArity::List`
/// (`@stream` on anything else is a semantic-check error, not something
/// this function needs to defend against).
pub(super) fn procedure_stream_item_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    procedure_item_type_tokens(type_ref, types, enum_names)
}
