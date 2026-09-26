//! The generated `envelope_layer(envelope, policy, audience)` convenience
//! (cratestack#1006, API-review decision 2026-09-26): an
//! `EnvelopeLayerBuilder` already set for this schema's transport, its
//! `ROUTE_TRANSPORTS` and its `SCHEMA_SHA256_BYTES`, so a REST schema's
//! layer cannot be built as RPC (or over another schema's descriptors).
//!
//! Emitted as a call to the facade's `__envelope_layer_fn!` macro, not as
//! the function itself (second-review decision S-1, 2026-09-26). This crate
//! is a proc-macro, so a feature of its own would be unified across the
//! whole build: one crate turning it on would change what every other
//! server schema in the build expands to, including one compiled through a
//! facade without the layer. Each facade (`cratestack-pg`,
//! `cratestack-api`) defines `__envelope_layer_fn!` under its own
//! `envelope` feature, expanding to the function or to nothing.

use quote::quote;

pub(super) fn build(is_rpc: bool) -> proc_macro2::TokenStream {
    let transport = if is_rpc {
        quote! { rpc }
    } else {
        quote! { rest }
    };
    quote! {
        ::cratestack::__envelope_layer_fn!(#transport);
    }
}
