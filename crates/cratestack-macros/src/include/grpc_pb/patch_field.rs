//! Per-field patch-field rendering for `Update<M>Input` — split out of
//! `update_message.rs` to stay under the repo's 200-LoC file convention.
//! See that module's doc for the full "why proto3 `optional` can't
//! express explicit-clear" design note this file's logic implements; this
//! file is the mechanical per-field-kind dispatch + the shared
//! presence-wrapping shape both directions (`From`/`TryFrom`) share.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::ident;

use super::scalar::{domain_from_wire_expr, scalar_wire, wire_from_domain_expr};

pub(super) struct PatchFieldPlan {
    pub(super) prost_field: proc_macro2::TokenStream,
    pub(super) from_domain_init: proc_macro2::TokenStream,
    pub(super) try_from_wire_let: proc_macro2::TokenStream,
}

/// Unlike `message.rs::render_field`, every case here ends up structurally
/// the same: `optional <wire> field = N` on the wire (single presence
/// level, patch-touched-or-not), converted to `Option<DomainInner>` where
/// `DomainInner` is `T` for a domain-`Required` field or `Option<T>` for a
/// domain-`Optional`/`List` one. List-arity patch fields (replace the
/// whole list, or don't) use `repeated` on the wire with the same
/// touched/absent ambiguity `message.rs` already documents for ordinary
/// list fields (`docs/design/protobuf.md` §4.4's exception) — here that
/// ambiguity means "absent" and "explicitly set to an empty list" are the
/// same wire bytes, in addition to "untouched".
pub(super) fn render_patch_field(
    owner: &str,
    field: &Field,
    number: i32,
    enum_names: &BTreeSet<&str>,
) -> PatchFieldPlan {
    let field_ident = ident(&field.name);
    let field_name = field.name.as_str();
    let type_name = field.ty.name.as_str();
    let number_lit = proc_macro2::Literal::i32_unsuffixed(number);
    let arity = field.ty.arity;

    if let Some(wire) = scalar_wire(type_name) {
        let rust_inner = &wire.rust_type;
        let kind = &wire.prost_kind;
        let to_domain = move |expr| domain_from_wire_expr(type_name, expr, owner, field_name);
        let to_wire = move |expr| wire_from_domain_expr(type_name, expr);
        render_patch_field_generic(
            &field_ident,
            number_lit,
            arity,
            quote! { #kind, optional },
            quote! { #kind, repeated },
            rust_inner.clone(),
            to_domain,
            to_wire,
        )
    } else if enum_names.contains(type_name) {
        let enum_ident = ident(type_name);
        let domain_enum_path = quote! { super::super::#enum_ident };
        render_patch_field_generic(
            &field_ident,
            number_lit,
            arity,
            quote! { int32, optional },
            quote! { int32, repeated },
            quote! { i32 },
            move |expr| {
                quote! { <#domain_enum_path as ::core::convert::TryFrom<i32>>::try_from(#expr) }
            },
            move |expr| quote! { i32::from(&(#expr)) },
        )
    } else {
        // Message-reference patch field (rare — a relation field surviving
        // `scalar_model_fields` filtering doesn't happen; this arm exists
        // for a `type`-typed field, which CAN appear on an update input).
        let message_ident = ident(type_name);
        let domain_message_path = quote! { super::super::#message_ident };
        render_patch_field_generic(
            &field_ident,
            number_lit,
            arity,
            quote! { message, optional, boxed },
            quote! { message, repeated },
            quote! { Box<#message_ident> },
            move |expr| quote! { #domain_message_path::try_from(*(#expr)) },
            move |expr| quote! { Box::new(#message_ident::from(&(#expr))) },
        )
    }
}

/// Shared shape for every patch-field kind: the pb wire is always a
/// single-presence `optional`/`repeated`; the domain side is
/// `Option<Inner>` for a domain-`Required`/`List` field and
/// `Option<Option<Inner>>` for a domain-`Optional` one (patch-presence
/// wrapping field-nullability) — see `update_message.rs`'s module doc for
/// why `Some(None)` (explicit clear) is never produced/round-tripped.
/// `domain_expr` builds the decode expression from an inner wire value
/// expr; for the message-reference kind it already returns a `Result`
/// (via `TryFrom`), same as every other kind after `?`. `to_wire_expr`
/// builds the encode expression (always infallible) from an owned inner
/// domain value expr — mirrors `message.rs::render_field`'s `to_wire`
/// closure.
#[allow(clippy::too_many_arguments)]
fn render_patch_field_generic(
    field_ident: &syn::Ident,
    number_lit: proc_macro2::Literal,
    arity: TypeArity,
    optional_attr: proc_macro2::TokenStream,
    repeated_attr: proc_macro2::TokenStream,
    rust_inner: proc_macro2::TokenStream,
    domain_expr: impl Fn(proc_macro2::TokenStream) -> proc_macro2::TokenStream,
    to_wire_expr: impl Fn(proc_macro2::TokenStream) -> proc_macro2::TokenStream,
) -> PatchFieldPlan {
    if arity == TypeArity::List {
        let to_domain = domain_expr(quote! { raw });
        let to_wire = to_wire_expr(quote! { inner });
        return PatchFieldPlan {
            prost_field: quote! {
                #[prost(#repeated_attr, tag = #number_lit)]
                pub #field_ident: Vec<#rust_inner>,
            },
            from_domain_init: quote! {
                #field_ident: value.#field_ident.clone().unwrap_or_default()
                    .into_iter()
                    .map(|inner| #to_wire)
                    .collect(),
            },
            try_from_wire_let: quote! {
                let #field_ident = if value.#field_ident.is_empty() {
                    None
                } else {
                    Some(value.#field_ident
                        .into_iter()
                        .map(|raw| -> ::core::result::Result<_, ::cratestack::CoolError> { #to_domain })
                        .collect::<::core::result::Result<Vec<_>, ::cratestack::CoolError>>()?)
                };
            },
        };
    }

    let to_domain = domain_expr(quote! { raw });
    let to_wire = to_wire_expr(quote! { inner });
    let prost_field = quote! {
        #[prost(#optional_attr, tag = #number_lit)]
        pub #field_ident: Option<#rust_inner>,
    };

    if arity == TypeArity::Optional {
        // Domain field type is `Option<Option<Inner>>`: patch-presence
        // (outer) wraps field-nullability (inner). `Some(None)` (explicit
        // clear) is never produced on decode, and collapses to wire-absent
        // on encode via `.flatten()` — see the module doc.
        PatchFieldPlan {
            prost_field,
            from_domain_init: quote! {
                #field_ident: value.#field_ident.clone().flatten().map(|inner| #to_wire),
            },
            try_from_wire_let: quote! {
                let #field_ident = match value.#field_ident {
                    None => None,
                    Some(raw) => Some(Some(#to_domain?)),
                };
            },
        }
    } else {
        // Domain field type is `Option<Inner>`: patch-presence only.
        PatchFieldPlan {
            prost_field,
            from_domain_init: quote! {
                #field_ident: value.#field_ident.clone().map(|inner| #to_wire),
            },
            try_from_wire_let: quote! {
                let #field_ident = value.#field_ident
                    .map(|raw| -> ::core::result::Result<_, ::cratestack::CoolError> { #to_domain })
                    .transpose()?;
            },
        }
    }
}
