//! `struct_field_type`/`struct_field_definition` — the field-token pair
//! every struct-shaped emitter (model structs, CRUD inputs) builds its
//! fields from. Split out of `struct_only.rs` per the repo's 200-LoC file
//! convention; re-exported from there so `crate::model::struct_only::*`
//! call sites (`crate::builder::fields`, `crate::view::struct_only`) don't
//! need to know about the split.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::{doc_attrs, ident, is_server_only_field, rust_type_tokens_with_scope};

/// The exact type tokens [`struct_field_definition`] puts on the field.
/// Extracted so the typestate builder emitter can take a setter argument
/// of precisely the field's own type without re-deriving it (and drifting
/// from it) — see [`crate::builder`].
pub(crate) fn struct_field_type(
    field: &Field,
    wrap_for_patch: bool,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let base_type = if enum_names.contains(field.ty.name.as_str()) {
        let enum_ident = ident(&field.ty.name);
        match field.ty.arity {
            TypeArity::Required => quote! { super::types::#enum_ident },
            TypeArity::Optional => quote! { Option<super::types::#enum_ident> },
            TypeArity::List => quote! { Vec<super::types::#enum_ident> },
        }
    } else {
        rust_type_tokens_with_scope(&field.ty, true)
    };
    if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    }
}

pub(crate) fn struct_field_definition(
    field: &Field,
    wrap_for_patch: bool,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let field_type = struct_field_type(field, wrap_for_patch, enum_names);
    // `@server_only` fields stay readable inside server code (SQLx populates
    // them via FromRow, which doesn't go through serde) but are masked from
    // both outbound JSON and inbound deserialization. The default value is
    // used if a client somehow sends one — banks shouldn't rely on that;
    // it's a defence-in-depth seam.
    let serde_attr = if is_server_only_field(field) {
        quote! { #[serde(skip_serializing, default)] }
    } else if wrap_for_patch && matches!(field.ty.arity, TypeArity::Optional) {
        // A nullable column on an update input is `Option<Option<T>>`:
        // outer = "did this patch touch the field at all", inner = "the
        // new value, or NULL to clear". serde-derive's blanket
        // `Option<T>: Deserialize` only ever peels the outer layer — an
        // absent key AND an explicit JSON/CBOR `null` both collapse to
        // outer `None`, so "clear this column" was unreachable over the
        // wire and silently no-op'd (cratestack#567).
        // `deserialize_double_option` recurses into the inner `Option`
        // instead; `default` is required alongside it because a custom
        // `deserialize_with` opts the field out of serde-derive's own
        // implicit "missing `Option<T>` field defaults to `None`".
        // `skip_serializing_if` is the matching fix for the *outbound*
        // side: every generated client builds a full input struct with
        // `..Default::default()` and serializes the whole thing, so
        // without this an untouched field would serialize as `null` —
        // indistinguishable from (and, after the deserialize fix, wrongly
        // interpreted as) an explicit clear. See `cratestack_core::patch`
        // for the full write-up.
        quote! {
            #[serde(
                default,
                deserialize_with = "::cratestack::deserialize_double_option",
                skip_serializing_if = "::std::option::Option::is_none"
            )]
        }
    } else if wrap_for_patch && matches!(field.ty.arity, TypeArity::List) {
        // A list column on an update input is `Option<Vec<T>>` — only one
        // `Option` layer, because arity `List` already turned the base
        // type into `Vec<T>` before patch-wrapping added the "did the
        // caller touch this field" layer on top (unlike `Optional`, which
        // contributes an `Option` of its own that patch-wrapping then
        // wraps a second time). There is no inner null-vs-empty state to
        // recurse into — the bulk setter's argument is a bare `Vec<T>`,
        // never `Option<Vec<T>>` — so serde-derive's own blanket
        // `Option<T>: Deserialize` (missing key -> `None`) is already
        // correct and no custom `deserialize_with` is needed.
        // `skip_serializing_if` is still required on the outbound side,
        // for the same reason as the `Optional` branch above: every
        // generated client builds a full input struct with
        // `..Default::default()` and serializes the whole thing, so
        // without this an untouched list field would serialize as `null`
        // — indistinguishable from "touched and set to an empty list".
        quote! {
            #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        }
    } else if matches!(field.ty.arity, TypeArity::Optional) && !wrap_for_patch {
        // Generated model structs declare Optional fields as `Option<T>`,
        // but the wire projection strips `null` map entries before the
        // codec sees them (CBOR/minicbor-serde encodes `Value::Null` as an
        // empty array, which would corrupt round-trips). `#[serde(default)]`
        // lets the client struct accept "missing field" as `None`,
        // restoring the round-trip without changing the wire format.
        quote! { #[serde(default)] }
    } else {
        quote! {}
    };

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}
