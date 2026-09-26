//! The generated `envelope_layer(envelope, policy, audience)` convenience
//! (cratestack#1006, API-review decision 2026-09-26): an
//! `EnvelopeLayerBuilder` already set for this schema's transport, its
//! `ROUTE_TRANSPORTS` and its `SCHEMA_SHA256_BYTES`, so a REST schema's
//! layer cannot be built as RPC (or over another schema's descriptors).
//!
//! Emitted only when this crate has its `envelope` feature, which the
//! `cratestack-pg` / `cratestack-api` facades forward from their own
//! `envelope` (and `cose`) feature: the function names
//! `::cratestack::envelope_layer`, which only exists then. Same
//! `cfg!(feature = ..)`-against-this-crate mechanism as `mcp`.

use quote::quote;

pub(super) fn build(is_rpc: bool) -> proc_macro2::TokenStream {
    if !cfg!(feature = "envelope") {
        return quote! {};
    }
    let transport = if is_rpc {
        quote! { .rpc("") }
    } else {
        quote! { .rest("", ROUTE_TRANSPORTS) }
    };
    quote! {
        /// The envelope layer for this schema's router (ADR 0006,
        /// cratestack#1006): `envelope` opens and seals, `policy` picks
        /// each op's mode, `audience` is this service's configured id. The
        /// transport, the route descriptors and the schema digest are this
        /// schema's; the body cap is the default, which fits the routers'
        /// default body limit (raise it with `max_body_bytes` if the router
        /// gets a larger one). Mounted under a prefix, add
        /// `.mount_prefix("/api")`; then `.build()`, and apply it as the
        /// router's last `.layer(..)`.
        pub fn envelope_layer(
            envelope: impl ::cratestack::envelope_layer::ServerEnvelope,
            policy: impl ::cratestack::envelope_layer::EnvelopePolicy,
            audience: impl ::core::convert::Into<::std::string::String>,
        ) -> ::cratestack::envelope_layer::EnvelopeLayerBuilder {
            ::cratestack::envelope_layer::EnvelopeLayer::builder(
                envelope,
                audience,
                super::SCHEMA_SHA256_BYTES,
            )
            .policy(policy)
            #transport
            .max_body_bytes(::cratestack::envelope_layer::DEFAULT_MAX_BODY_BYTES)
        }
    }
}
