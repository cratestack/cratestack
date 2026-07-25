//! `Update<M>Input` pb mirror. Split out of `message.rs` because the
//! domain `Update<M>Input` struct has an extra `Option<_>` presence layer
//! ("was this field even sent in the patch?") on top of each field's own
//! arity — `struct_field_definition(field, wrap_for_patch = true, ..)` in
//! `crate::model::struct_only` wraps every field in `Option<_>`
//! unconditionally, so a domain-`Optional` field becomes `Option<Option<T>>`
//! (patch-presence outer, field-nullability inner) while a domain-`Required`
//! field becomes plain `Option<T>` (patch-presence only).
//!
//! **Judgment call, documented per ticket #171's ask:** proto3 `optional`
//! expresses exactly one presence bit. This module uses it for
//! patch-presence ("was this field touched?"); it cannot also express
//! "touched and explicitly set to null" for a domain-`Optional` field —
//! there is no wire-level way to distinguish "clear this field" from
//! "leave it alone" here. `TryFrom` therefore only ever produces `None`
//! (untouched) or `Some(Some(v))` (set to `v`) for those fields, never
//! `Some(None)` (explicit clear) — a real, narrower-than-REST/RPC
//! limitation of the gRPC binding specifically. A future revision could
//! close this with a `oneof { google.protobuf.Empty clear = N; T set = M; }`
//! wrapper per nullable field; not attempted here given the ticket's
//! already-large scope (see the crate README's status note).
//!
//! Separately: `cratestack-proto`'s already-shipped `synth.rs` numbers
//! `Update<M>Input` from `scalar_model_fields` filtered only by
//! `!is_primary_key` — it does **not** exclude `@readonly`/`@server_only`/
//! `@version` fields the way the Rust domain `Update<M>Input` struct does
//! (`crate::model::inputs::update_input_fields`). That means the committed
//! lock's `Update<M>Input` entry can contain field-name keys with no
//! matching domain struct field. This module only emits pb fields for the
//! **domain struct's** field set (a subset of the lock's), looking each
//! one's number up by name — the lock simply has spare, unused entries for
//! the rest. A protoc-generated client that encodes one of those extra
//! field numbers has its bytes silently dropped by prost's decoder (an
//! unknown field on a mirror struct that doesn't declare that tag) —
//! forward-compatible protobuf behavior, and arguably the *correct*
//! outcome here (a client can't mutate a `@readonly`/`@server_only`/
//! `@version` field over gRPC any more than it can over REST/RPC).

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::ident;

use super::scalar::{domain_from_wire_expr, scalar_wire};

pub(super) struct RenderedUpdateMessage {
    pub(super) tokens: proc_macro2::TokenStream,
}

/// `message_name`: `Update<Model>Input`. `domain_path`: tokens resolving
/// to the existing domain struct (`super::super::Update<Model>Input`).
/// `fields`: the domain struct's own field set — already filtered by the
/// caller to match `crate::model::inputs::update_input_fields` (PK,
/// `@readonly`, `@server_only`, `@version` excluded). `numbers`: the raw
/// `[messages.Update<Model>Input]` lock entry — a superset of `fields`,
/// per the module doc; only entries matching a name in `fields` are used.
pub(super) fn render_update_message(
    message_name: &str,
    domain_path: proc_macro2::TokenStream,
    fields: &[&Field],
    numbers: &BTreeMap<String, i32>,
    enum_names: &BTreeSet<&str>,
) -> Result<RenderedUpdateMessage, String> {
    let ident_tok = ident(message_name);
    let mut prost_fields = Vec::with_capacity(fields.len());
    let mut try_from_wire_lets = Vec::with_capacity(fields.len());
    let mut try_from_wire_inits = Vec::with_capacity(fields.len());

    for field in fields {
        let number = *numbers
            .get(&field.name)
            .ok_or_else(|| format!("no `.pb.lock` entry for `{message_name}.{}`", field.name))?;
        let plan = render_patch_field(message_name, field, number, enum_names);
        prost_fields.push(plan.prost_field);
        try_from_wire_lets.push(plan.try_from_wire_let);
        let field_ident = ident(&field.name);
        try_from_wire_inits.push(quote! { #field_ident, });
    }

    let tokens = quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #(#prost_fields)*
        }

        // Patches are gRPC-request-only on this binding today — no code
        // path serializes a domain patch struct back out as a pb message
        // (the mirror layer's usual `From<&Domain> for Mirror` direction),
        // so only the decode (`TryFrom<Mirror> for Domain`) direction is
        // implemented. Add `From<&#domain_path> for #ident_tok` if a
        // future caller needs to echo a patch back on the wire.
        impl ::core::convert::TryFrom<#ident_tok> for #domain_path {
            type Error = ::cratestack::CoolError;

            fn try_from(value: #ident_tok) -> ::core::result::Result<Self, Self::Error> {
                #(#try_from_wire_lets)*
                Ok(Self {
                    #(#try_from_wire_inits)*
                })
            }
        }
    };

    Ok(RenderedUpdateMessage { tokens })
}

struct PatchFieldPlan {
    prost_field: proc_macro2::TokenStream,
    try_from_wire_let: proc_macro2::TokenStream,
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
fn render_patch_field(
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
        render_patch_field_generic(
            &field_ident,
            number_lit,
            arity,
            quote! { #kind, optional },
            quote! { #kind, repeated },
            rust_inner.clone(),
            to_domain,
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
        )
    }
}

/// Shared shape for every patch-field kind: the pb wire is always a
/// single-presence `optional`/`repeated`; the domain side is
/// `Option<Inner>` for a domain-`Required`/`List` field and
/// `Option<Option<Inner>>` for a domain-`Optional` one (patch-presence
/// wrapping field-nullability) — see the module doc for why `Some(None)`
/// (explicit clear) is never produced. `domain_expr` builds the decode
/// expression from an inner wire value expr; for the message-reference
/// kind it already returns a `Result` (via `TryFrom`), same as every other
/// kind after `?`.
fn render_patch_field_generic(
    field_ident: &syn::Ident,
    number_lit: proc_macro2::Literal,
    arity: TypeArity,
    optional_attr: proc_macro2::TokenStream,
    repeated_attr: proc_macro2::TokenStream,
    rust_inner: proc_macro2::TokenStream,
    domain_expr: impl Fn(proc_macro2::TokenStream) -> proc_macro2::TokenStream,
) -> PatchFieldPlan {
    if arity == TypeArity::List {
        let to_domain = domain_expr(quote! { raw });
        return PatchFieldPlan {
            prost_field: quote! {
                #[prost(#repeated_attr, tag = #number_lit)]
                pub #field_ident: Vec<#rust_inner>,
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
    let prost_field = quote! {
        #[prost(#optional_attr, tag = #number_lit)]
        pub #field_ident: Option<#rust_inner>,
    };

    if arity == TypeArity::Optional {
        // Domain field type is `Option<Option<Inner>>`: patch-presence
        // (outer) wraps field-nullability (inner). `Some(None)` (explicit
        // clear) is never produced on decode — see the module doc.
        PatchFieldPlan {
            prost_field,
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
            try_from_wire_let: quote! {
                let #field_ident = value.#field_ident
                    .map(|raw| -> ::core::result::Result<_, ::cratestack::CoolError> { #to_domain })
                    .transpose()?;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: &str, arity: TypeArity) -> Field {
        Field {
            docs: vec![],
            name: name.to_owned(),
            name_span: cratestack_core::SourceSpan {
                start: 0,
                end: 0,
                line: 0,
            },
            ty: cratestack_core::TypeRef {
                name: ty.to_owned(),
                name_span: cratestack_core::SourceSpan {
                    start: 0,
                    end: 0,
                    line: 0,
                },
                arity,
                generic_args: vec![],
            },
            attributes: Vec::new(),
            span: cratestack_core::SourceSpan {
                start: 0,
                end: 0,
                line: 0,
            },
        }
    }

    #[test]
    fn required_field_wraps_in_single_option() {
        let name_field = field("name", "String", TypeArity::Required);
        let fields = vec![&name_field];
        let numbers = BTreeMap::from([("name".to_owned(), 1)]);
        let rendered = render_update_message(
            "UpdateWidgetInput",
            quote! { super::super::UpdateWidgetInput },
            &fields,
            &numbers,
            &BTreeSet::new(),
        )
        .expect("should render");
        let rendered_str = rendered.tokens.to_string();
        assert!(rendered_str.contains("pub name : Option < String >"));
        // Required-arity domain field: `Option<T>` on decode, no
        // double-wrap — presence-only.
        assert!(!rendered_str.contains("Some (Some ("));
    }

    #[test]
    fn optional_field_double_wraps_on_decode() {
        let email_field = field("email", "String", TypeArity::Optional);
        let fields = vec![&email_field];
        let numbers = BTreeMap::from([("email".to_owned(), 1)]);
        let rendered = render_update_message(
            "UpdateWidgetInput",
            quote! { super::super::UpdateWidgetInput },
            &fields,
            &numbers,
            &BTreeSet::new(),
        )
        .expect("should render");
        let rendered_str = rendered.tokens.to_string();
        // Optional-arity domain field: `Option<Option<T>>` — patch
        // presence wraps field nullability.
        assert!(rendered_str.contains("Some (Some ("));
        assert!(!rendered_str.contains("Some (None)"));
    }

    #[test]
    fn missing_lock_entry_is_reported_not_panicked() {
        let name_field = field("name", "String", TypeArity::Required);
        let fields = vec![&name_field];
        let result = render_update_message(
            "UpdateWidgetInput",
            quote! { super::super::UpdateWidgetInput },
            &fields,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        assert!(result.is_err());
    }
}
