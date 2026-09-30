//! The per-op contract constants every `include_*_schema!` module emits
//! (cratestack#1123, EXT-14; binding version 2).
//!
//! - `OP_CONTRACTS`: the digest each call binds, keyed by op key (the RPC
//!   `op_id`, `"<METHOD> <route template>"` on REST, plus `batch` for
//!   `transport rpc`), in key order (nothing relies on it). What the generated client stamps, and
//!   what the layer's accepted table starts from.
//! - `CLIENT_CONTRACT_SHA256(_BYTES)`: the whole-contract build identity.
//! - `ACCEPTED_CONTRACTS` (server module only): per key, the digests a
//!   request may bind, current first, then older compatible ones newest
//!   first. One member per key without a lock; with `contracts = "..."` it
//!   is `ContractLock::accepted`'s answer (an incompatible locked entry is a
//!   compile error, see `super::lock_arg`), so the layer that reads it never
//!   changes.
//!
//! The first two come from `cratestack_core::bound_contracts`, the function
//! the CLI prints from, so a table and its display cannot disagree.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

#[derive(Default)]
pub(super) struct ContractConsts {
    ops: Vec<(String, [u8; 32])>,
    /// Per key, the digests a server accepts; the current one first.
    accepted: Vec<(String, Vec<[u8; 32]>)>,
    client: [u8; 32],
}

impl ContractConsts {
    pub(super) fn from_schema(schema: &cratestack_core::Schema) -> Self {
        let ops = cratestack_core::bound_contracts(schema);
        Self {
            accepted: ops.iter().map(|(k, d)| (k.clone(), vec![*d])).collect(),
            ops,
            client: cratestack_core::client_contract_digest(schema),
        }
    }

    /// Accept, per key, the digests of `table` (a `ContractLock::accepted`
    /// answer) instead of the current one alone.
    pub(super) fn with_accepted(mut self, table: Vec<(String, Vec<[u8; 32]>)>) -> Self {
        self.accepted = table;
        self
    }

    /// `pub const ACCEPTED_CONTRACTS`: each key's accepted digests.
    pub(super) fn accepted(&self) -> TokenStream {
        let rows = self.accepted.iter().map(|(key, digests)| {
            let digests = digests.iter().map(|digest| {
                let digest = digest.iter();
                quote! { [#(#digest),*] }
            });
            quote! { (#key, &[#(#digests),*]) }
        });
        quote! {
            /// Per op key (and `batch`), the op-contract digests a signed
            /// request may bind: the current one first, then older ones the
            /// server still accepts, newest first. The generated
            /// `axum::envelope_layer` reads it (cratestack#1123).
            pub const ACCEPTED_CONTRACTS: &[(&str, &[[u8; 32]])] = &[#(#rows),*];
        }
    }
}

impl ToTokens for ContractConsts {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let rows = self.ops.iter().map(|(key, digest)| {
            let digest = digest.iter();
            quote! { (#key, [#(#digest),*]) }
        });
        let hex = cratestack_core::digest_hex(&self.client);
        let client = self.client.iter();
        tokens.extend(quote! {
            /// The digest each call binds into a signed message (binding
            /// version 2; ADR 0006 §4, cratestack#1123): per op key (the RPC
            /// `op_id`, or `"<METHOD> <route template>"` on REST, plus
            /// `batch` for `transport rpc`), in key order (nothing relies on it).
            /// It moves only
            /// when that op's wire shape does.
            pub const OP_CONTRACTS: &[(&str, [u8; 32])] = &[#(#rows),*];
            /// Hex of [`CLIENT_CONTRACT_SHA256_BYTES`]: a hash over every
            /// op's digest, moving when any op's contract does. A build
            /// identity for tooling, and what a signed `/rpc/batch` binds
            /// until batch frames carry their own digests.
            pub const CLIENT_CONTRACT_SHA256: &str = #hex;
            /// The whole-contract digest as its 32 raw bytes.
            pub const CLIENT_CONTRACT_SHA256_BYTES: [u8; 32] = [#(#client),*];
        });
    }
}

#[cfg(test)]
mod tests {
    use super::ContractConsts;

    fn consts() -> ContractConsts {
        ContractConsts {
            ops: vec![("model.W.list".to_owned(), [7; 32])],
            accepted: vec![("model.W.list".to_owned(), vec![[7; 32], [5; 32]])],
            client: [9; 32],
        }
    }

    #[test]
    fn the_tables_and_the_identity_are_emitted() {
        let emitted = {
            let consts = consts();
            quote::quote!(#consts).to_string()
        };
        assert!(emitted.contains("pub const OP_CONTRACTS"), "{emitted}");
        assert!(emitted.contains("\"model.W.list\""), "{emitted}");
        assert!(
            emitted.contains("pub const CLIENT_CONTRACT_SHA256_BYTES : [u8 ; 32]"),
            "{emitted}"
        );
        assert!(emitted.contains(&"09".repeat(32)), "{emitted}");
    }

    #[test]
    fn accepted_lists_the_current_digest_then_the_locked_ones() {
        let emitted = consts().accepted().to_string();
        assert!(
            emitted.contains("pub const ACCEPTED_CONTRACTS : & [(& str , & [[u8 ; 32]])]"),
            "{emitted}"
        );
        let (seven, five) = (emitted.find("7u8").unwrap(), emitted.find("5u8").unwrap());
        assert!(seven < five, "current first: {emitted}");
    }
}
