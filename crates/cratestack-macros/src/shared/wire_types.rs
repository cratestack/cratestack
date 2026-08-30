//! Type-token resolution for the server composer's dedicated `wire`
//! module (`crate::computed::wire`) — the wire-shape mirror of every
//! `@computed`-bearing owner, used to fix the self/peer-calling client's
//! silent-drop bug documented in `docs/design/computed-fields.md`'s
//! "Exclusions" section.
//!
//! [`rust_type_tokens_with_wire_scope`] is deliberately a *sibling* of
//! [`super::rust_type_tokens_with_scope`], not a parameter added to it:
//! that function has ~20 call sites across this crate, and every one of
//! them wants the plain `super::<Ident>` resolution unchanged. Only the
//! two wire-struct emitters (`generate_wire_type_struct`,
//! `generate_wire_model_struct`) need the "a bearing owner's field
//! resolves to the sibling wire struct" branch, so it's kept fully
//! separate — duplicating the small scalar-name match arm rather than
//! risking a signature change rippling through call sites that have
//! nothing to do with `@computed`. `bearing.rs`'s module doc documents
//! the same duplicate-rather-than-share tradeoff for the analogous
//! "recompute a schema-wide fixed point locally" case.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity, TypeRef};
use quote::quote;

use super::{doc_attrs, ident};

/// Like [`super::rust_type_tokens_with_scope`] (always `custom_in_super =
/// true`, matching the two wire emitters' own use of that function today),
/// except a name in `bearing` resolves to `super::wire::<Ident>` — the
/// sibling wire struct one level up from wherever this token stream is
/// spliced (`pub mod wire { ... }` sits directly under `cratestack_schema`,
/// the same nesting depth as `models`/`types`) — instead of the plain
/// `super::<Ident>` (the server-side struct, missing computed fields).
pub(crate) fn rust_type_tokens_with_wire_scope(
    type_ref: &TypeRef,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = rust_type_tokens_with_wire_scope(item, bearing);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
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
        "Vector" => quote! { Vec<f32> },
        // EWKB bytes on the wire, same as `Bytes` — see
        // `shared::types` for why spatial fields stay `Vec<u8>`.
        "Geography" | "Geometry" => quote! { Vec<u8> },
        other if bearing.contains(other) => {
            let wire_ident = ident(other);
            quote! { super::wire::#wire_ident }
        }
        other => {
            let plain_ident = ident(other);
            quote! { super::#plain_ident }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

/// [`super::field_type`]'s wire-scope counterpart — always
/// `wrap_for_patch = false` (wire structs are response-only, never a
/// patch/update input).
pub(crate) fn field_type_with_wire_scope(
    field: &Field,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    rust_type_tokens_with_wire_scope(&field.ty, bearing)
}

/// [`super::field_definition`]'s wire-scope counterpart, for
/// `generate_wire_type_struct` — mirrors [`super::field_definition`]'s
/// `custom_in_super = true` field-token shape exactly, substituting a
/// bearing field's type per [`rust_type_tokens_with_wire_scope`].
pub(crate) fn field_definition_with_wire_scope(
    field: &Field,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let field_type = field_type_with_wire_scope(field, bearing);
    // Mirrors [`super::field_definition`]'s `Bytes` handling — a wire
    // `type` struct is always `wrap_for_patch = false` (response-only),
    // so the shape it needs is the non-patch row of the table in
    // `super::bytes_serde` (cratestack#783).
    let serde_attr = super::bytes_serde_attr(&field.ty, false);

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}
