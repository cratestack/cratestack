//! The digest constants every `include_*_schema!` module emits.
//!
//! `SCHEMA_SHA256` (hex) predates signing and feeds the warn-only drift
//! header (#178); `SCHEMA_SHA256_BYTES` is the same whole-IR digest as raw
//! bytes. Neither is bound into a signed message any more: binding version
//! 2 binds the digest of the op being called (cratestack#1123, ADR 0006
//! §4), which [`ContractConsts`] emits as `OP_CONTRACTS`, with
//! `CLIENT_CONTRACT_SHA256(_BYTES)` the whole-contract build identity.
//! All are emitted by all three macros because both ends of a signed
//! exchange must rebuild the same binding: the server's envelope layer, and
//! the client (#1007) and embedded builds that seal requests for it.
//! Emitting bytes, rather than having each consumer hex-decode at runtime,
//! keeps a decode error (and a second place to get it wrong) out of the
//! signing path.
//!
//! Each pair comes from one value here, so the two spellings cannot
//! disagree.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::contracts::ContractConsts;

/// Emits `pub const SCHEMA_SHA256: &str` and `pub const SCHEMA_SHA256_BYTES:
/// [u8; 32]` when interpolated with `#`. A doc comment written just before
/// the interpolation attaches to the first, the hex constant, as it always
/// has; the bytes constant carries its own.
pub(super) struct SchemaShaConsts {
    hex: String,
    bytes: [u8; 32],
    contracts: ContractConsts,
}

impl SchemaShaConsts {
    /// Every digest of `schema`: the whole-IR `schema_digest` (the drift
    /// header's identity) and the per-op contract table.
    pub(super) fn from_schema(schema: &cratestack_core::Schema) -> Self {
        let mut consts = Self::from_digest(cratestack_core::schema_digest(schema));
        consts.contracts = ContractConsts::from_schema(schema);
        consts
    }

    /// `bytes` is `cratestack_core::schema_digest`'s output; the hex
    /// constant is derived from it here.
    fn from_digest(bytes: [u8; 32]) -> Self {
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            hex,
            bytes,
            contracts: ContractConsts::default(),
        }
    }

    /// Accept the digests a contract lock keeps, not just the current ones.
    pub(super) fn with_accepted(mut self, table: Vec<(String, Vec<[u8; 32]>)>) -> Self {
        self.contracts = self.contracts.with_accepted(table);
        self
    }

    /// `ACCEPTED_CONTRACTS`, for the server module only.
    pub(super) fn accepted(&self) -> TokenStream {
        self.contracts.accepted()
    }
}

impl ToTokens for SchemaShaConsts {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let hex = &self.hex;
        let bytes = self.bytes.iter();
        tokens.extend(quote! {
            pub const SCHEMA_SHA256: &str = #hex;
            /// `SCHEMA_SHA256` as its 32 raw bytes: the schema's canonical
            /// whole-IR identity, not a hash of its text (a comment or
            /// whitespace edit leaves it unchanged, cratestack#1065). It
            /// is **not** what a signed message binds since binding
            /// version 2: that is the called op's digest in
            /// `OP_CONTRACTS`.
            pub const SCHEMA_SHA256_BYTES: [u8; 32] = [#(#bytes),*];
        });
        self.contracts.to_tokens(tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaShaConsts;

    #[test]
    fn the_hex_is_the_digest_bytes_encoded() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x50;
        bytes[31] = 0x3e;
        let consts = SchemaShaConsts::from_digest(bytes);
        assert!(consts.hex.starts_with("5000"), "{}", consts.hex);
        assert!(consts.hex.ends_with("3e"), "{}", consts.hex);
        assert_eq!(consts.hex.len(), 64);
    }

    #[test]
    fn both_constants_are_emitted() {
        let consts = SchemaShaConsts::from_digest([0xe3; 32]);
        let emitted = quote::quote!(#consts).to_string();
        assert!(
            emitted.contains("pub const SCHEMA_SHA256 : & str"),
            "{emitted}"
        );
        assert!(
            emitted.contains("pub const SCHEMA_SHA256_BYTES : [u8 ; 32]"),
            "{emitted}"
        );
        assert!(emitted.contains("[227u8 , 227u8"), "{emitted}");
    }
}
