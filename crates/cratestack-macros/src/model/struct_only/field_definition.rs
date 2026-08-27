//! `struct_field_type`/`struct_field_definition` — the field-token pair
//! every struct-shaped emitter (model structs, CRUD inputs) builds its
//! fields from. Split out of `struct_only.rs` per the repo's 200-LoC file
//! convention; re-exported from there so `crate::model::struct_only::*`
//! call sites (`crate::builder::fields`, `crate::view::struct_only`) don't
//! need to know about the split.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::{
    doc_attrs, ident, is_server_only_field, rust_type_tokens_with_scope,
    rust_type_tokens_with_wire_scope,
};

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
    // A `Bytes` field needs `deserialize_with` pointing at a deserializer
    // that accepts a CBOR byte string as well as the integer array every
    // deployed client sends (cratestack#783). Only the *argument* is taken
    // here, never `BytesSerde::args()`: every branch below that can host it
    // already emits its own `default`, and a duplicate is a compile error.
    // The one branch that already sets `deserialize_with` — the double
    // `Option` patch case — uses this as a *replacement*, which is why the
    // `Bytes` mapping in `bytes_serde` has its own double-option variant.
    let bytes_deserialize_with = crate::shared::bytes_deserialize_with(&field.ty, wrap_for_patch);
    // Pre-rendered with its leading comma so it can be appended to a
    // branch's argument list, or expand to nothing for every other type.
    let bytes_arg = match &bytes_deserialize_with {
        Some(argument) => quote! { , #argument },
        None => quote! {},
    };
    // `@server_only` fields stay readable inside server code (SQLx populates
    // them via FromRow, which doesn't go through serde) but are masked from
    // both outbound JSON and inbound deserialization. The default value is
    // used if a client somehow sends one — banks shouldn't rely on that;
    // it's a defence-in-depth seam.
    let serde_attr = if is_server_only_field(field) {
        quote! { #[serde(skip_serializing, default #bytes_arg)] }
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
        //
        // A `Bytes` field swaps in `deserialize_double_option_bytes`
        // instead of appending: `deserialize_double_option`'s `T:
        // Deserialize` bound resolves to `Vec<u8>`'s strict blanket impl,
        // which is exactly the byte-string-rejecting behaviour
        // cratestack#783 fixes.
        let double_option = bytes_deserialize_with.unwrap_or_else(|| {
            quote! { deserialize_with = "::cratestack::deserialize_double_option" }
        });
        quote! {
            #[serde(
                default,
                #double_option,
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
        // (A `Bytes` list does need one, for the element shape rather than
        // the `Option` layer — see cratestack#783.)
        quote! {
            #[serde(default, skip_serializing_if = "::std::option::Option::is_none" #bytes_arg)]
        }
    } else if wrap_for_patch && matches!(field.ty.arity, TypeArity::Required) {
        // A required-arity column on an update input is `Option<T>` — the
        // sole `Option` layer is patch-wrapping's own "did the caller touch
        // this field" marker; there's no nullable-column inner layer to
        // recurse into (that's the `Optional`-arity branch above), so
        // serde-derive's blanket `Option<T>: Deserialize` (missing key ->
        // `None`) is already correct and no custom `deserialize_with` is
        // needed. `skip_serializing_if` is still required on the outbound
        // side, for the same reason as the `Optional`/`List` branches
        // above: every generated client builds a full input struct with
        // `..Default::default()` and serializes the whole thing, so
        // without this an untouched required-arity field serialized as
        // `null` — the one arity #567/#662 didn't cover (cratestack#663).
        // (A `Bytes` field still needs one, for the wire shape of the
        // value rather than the `Option` layer — see cratestack#783.)
        quote! {
            #[serde(default, skip_serializing_if = "::std::option::Option::is_none" #bytes_arg)]
        }
    } else if matches!(field.ty.arity, TypeArity::Optional) && !wrap_for_patch {
        // Generated model structs declare Optional fields as `Option<T>`,
        // but the wire projection strips `null` map entries before the
        // codec sees them (CBOR/minicbor-serde encodes `Value::Null` as an
        // empty array, which would corrupt round-trips). `#[serde(default)]`
        // lets the client struct accept "missing field" as `None`,
        // restoring the round-trip without changing the wire format.
        quote! { #[serde(default #bytes_arg)] }
    } else if let Some(argument) = &bytes_deserialize_with {
        // Required- or list-arity `Bytes`, unwrapped: no `Option` layer,
        // so no `default` — just the leniency (cratestack#783).
        quote! { #[serde(#argument)] }
    } else {
        quote! {}
    };

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}

/// [`struct_field_type`]'s wire-scope counterpart, for
/// `generate_wire_model_struct` (`crate::model::struct_only`) — mirrors
/// `struct_field_type`'s `wrap_for_patch = false` shape (a wire model
/// struct is response-only) exactly, including the enum special case
/// (`super::types::<Enum>` — enums are never computed-bearing, so this
/// branch needs no substitution), and only changes the non-enum "other"
/// resolution: [`rust_type_tokens_with_wire_scope`] redirects a bearing
/// field to the sibling `super::wire::<Owner>` instead of the plain
/// server-side `super::<Owner>`.
pub(crate) fn struct_field_type_with_wire_scope(
    field: &Field,
    enum_names: &BTreeSet<&str>,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if enum_names.contains(field.ty.name.as_str()) {
        let enum_ident = ident(&field.ty.name);
        return match field.ty.arity {
            TypeArity::Required => quote! { super::types::#enum_ident },
            TypeArity::Optional => quote! { Option<super::types::#enum_ident> },
            TypeArity::List => quote! { Vec<super::types::#enum_ident> },
        };
    }
    rust_type_tokens_with_wire_scope(&field.ty, bearing)
}

/// [`struct_field_definition`]'s wire-scope counterpart. The `serde_attr`
/// selection only ever reaches the `wrap_for_patch = false` branches
/// (wire model structs never wrap for patch), so this mirrors just those.
pub(crate) fn struct_field_definition_with_wire_scope(
    field: &Field,
    enum_names: &BTreeSet<&str>,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let field_type = struct_field_type_with_wire_scope(field, enum_names, bearing);
    // Wire model structs are always `wrap_for_patch = false`, so the
    // `Bytes` shapes here are the non-patch row of `bytes_serde`'s table.
    let bytes_deserialize_with = crate::shared::bytes_deserialize_with(&field.ty, false);
    let bytes_arg = match &bytes_deserialize_with {
        Some(argument) => quote! { , #argument },
        None => quote! {},
    };
    let serde_attr = if is_server_only_field(field) {
        quote! { #[serde(skip_serializing, default #bytes_arg)] }
    } else if matches!(field.ty.arity, TypeArity::Optional) {
        quote! { #[serde(default #bytes_arg)] }
    } else if let Some(argument) = &bytes_deserialize_with {
        quote! { #[serde(#argument)] }
    } else {
        quote! {}
    };

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}
