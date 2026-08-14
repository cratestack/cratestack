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
//!
//! **Encode direction (`From<&Domain> for Mirror`), added for ticket
//! #209:** originally this module only emitted the decode
//! (`TryFrom<Mirror> for Domain`) direction — the server never needed to
//! echo a patch back out as a pb message. The native Rust gRPC client
//! (`include::client::grpc`) is the first caller that does: it has to
//! encode a domain `Update<M>Input` patch into this same mirror struct to
//! send as `<Model>RpcUpdateInput.patch`. The encode side collapses
//! `Option<Option<Inner>>` (domain-`Optional` fields: patch-presence
//! wrapping field-nullability) to a single wire-presence bit via
//! `.flatten()` — `None` (untouched) and `Some(None)` (explicit clear)
//! both encode as wire-absent, the same collapsing the decode direction
//! already documents above (`Some(None)` is never *produced* by decode;
//! this is the mirror fact that it can't be *round-tripped* through encode
//! either — one limitation, stated once, holds in both directions).
//! Always safe to emit unconditionally regardless of caller: an unused
//! trait impl method is not a `dead_code`-lint hazard the way an unused
//! *inherent* method would be (rustc does not flag unused trait impls), so
//! the server composer emitting this too and never calling it costs
//! nothing.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Field;
use quote::quote;

use crate::shared::ident;

use super::patch_field::render_patch_field;

pub(crate) struct RenderedUpdateMessage {
    pub(crate) tokens: proc_macro2::TokenStream,
}

/// `message_name`: `Update<Model>Input`. `domain_path`: tokens resolving
/// to the existing domain struct (`super::super::Update<Model>Input`).
/// `fields`: the domain struct's own field set — already filtered by the
/// caller to match `crate::model::inputs::update_input_fields` (PK,
/// `@readonly`, `@server_only`, `@version` excluded). `numbers`: the raw
/// `[messages.Update<Model>Input]` lock entry — a superset of `fields`,
/// per the module doc; only entries matching a name in `fields` are used.
pub(crate) fn render_update_message(
    message_name: &str,
    domain_path: proc_macro2::TokenStream,
    fields: &[&Field],
    numbers: &BTreeMap<String, i32>,
    enum_names: &BTreeSet<&str>,
) -> Result<RenderedUpdateMessage, String> {
    let ident_tok = ident(message_name);
    let mut prost_fields = Vec::with_capacity(fields.len());
    let mut from_domain_inits = Vec::with_capacity(fields.len());
    let mut try_from_wire_lets = Vec::with_capacity(fields.len());
    let mut try_from_wire_inits = Vec::with_capacity(fields.len());

    for field in fields {
        let number = *numbers
            .get(&field.name)
            .ok_or_else(|| format!("no `.pb.lock` entry for `{message_name}.{}`", field.name))?;
        let plan = render_patch_field(message_name, field, number, enum_names);
        prost_fields.push(plan.prost_field);
        from_domain_inits.push(plan.from_domain_init);
        try_from_wire_lets.push(plan.try_from_wire_let);
        let field_ident = ident(&field.name);
        try_from_wire_inits.push(quote! { #field_ident, });
    }

    let tokens = quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #(#prost_fields)*
        }

        impl ::core::convert::From<&#domain_path> for #ident_tok {
            fn from(value: &#domain_path) -> Self {
                Self {
                    #(#from_domain_inits)*
                }
            }
        }

        impl ::core::convert::TryFrom<#ident_tok> for #domain_path {
            type Error = ::cratestack::CratestackError;

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

#[cfg(test)]
mod tests {
    use cratestack_core::TypeArity;

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
                int_args: Vec::new(),
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
