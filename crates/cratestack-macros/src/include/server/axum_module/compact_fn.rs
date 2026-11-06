//! The generated `compact_layer(body_limit_bytes)`: the compact-wire-keys
//! layer (`cratestack_axum::compact_layer`) over this schema's `KEY_TABLES`.
//! `router()` and `rpc_router()` mount it themselves, inside their
//! `DefaultBodyLimit`, so a layer a consumer adds around the router sees the
//! compact payload types the client sent. Emitted for every server schema,
//! REST or RPC, `db = Postgres` or `db = None`.

use quote::quote;

pub(super) fn build() -> proc_macro2::TokenStream {
    quote! {
        /// The compact wire keys layer for this schema (docs/design/
        /// compact-wire-keys.md): opens compact request bodies and answers
        /// compact to clients that ask for it with `Accept` and
        /// `Cratestack-Contract`. `body_limit_bytes` bounds a compact
        /// request and the named body it expands to; pass the same value as
        /// the router's. Already mounted by `router()` and `rpc_router()`;
        /// call this only when composing `model_router`/`procedure_router`
        /// yourself.
        pub fn compact_layer(
            body_limit_bytes: usize,
        ) -> ::cratestack::compact_layer::CompactLayer {
            ::cratestack::compact_layer::CompactLayer::from_artifact(
                super::KEY_TABLES,
                body_limit_bytes,
            )
            .expect("the generated KEY_TABLES artifact is valid")
        }
    }
}
