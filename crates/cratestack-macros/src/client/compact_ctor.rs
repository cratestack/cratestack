//! `Client::with_compact_keys`, emitted into both generated client flavours
//! (REST and RPC). Compact wire keys are opt-in: [`Client::new`] keeps
//! today's wire, this constructor turns the schema's `KEY_TABLES` on.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn compact_constructor() -> TokenStream {
    quote! {
        /// Like [`Self::new`], with compact wire keys on
        /// (`docs/design/compact-wire-keys.md`): the client offers
        /// `Accept: application/vnd.cratestack.compact+*` and the op's
        /// `Cratestack-Contract`, and sends compact request bodies once the
        /// server has answered that op compact. Against a server without
        /// the feature every call stays named.
        pub fn with_compact_keys(
            runtime: ::cratestack::client_rust::CratestackClient<C>,
        ) -> Self {
            Self::new(
                runtime
                    .with_compact_key_bytes(super::KEY_TABLES, super::OP_CONTRACTS)
                    .expect("the generated KEY_TABLES artifact is valid"),
            )
        }
    }
}
