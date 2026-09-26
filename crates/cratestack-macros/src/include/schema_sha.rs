//! The two schema-digest constants every `include_*_schema!` module emits.
//!
//! `SCHEMA_SHA256` (hex) predates signing and feeds the warn-only drift
//! header (#178). `SCHEMA_SHA256_BYTES` is the same digest as the raw 32
//! bytes a COSE envelope binds as the AAD's `schema_sha` element (ADR 0006
//! §4; `cratestack_core::Binding::schema_sha`). It is emitted by all three
//! macros (cratestack#1006) because both ends of a signed exchange must
//! rebuild the same binding: the server's envelope layer, and the client
//! (#1007) and embedded builds that seal requests for it. Emitting the
//! bytes, rather than having each consumer hex-decode `SCHEMA_SHA256` at
//! runtime, keeps a decode error (and a second place to get it wrong) out of
//! the signing path.
//!
//! Both come from one value here, so they cannot disagree.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

/// Emits `pub const SCHEMA_SHA256: &str` and `pub const SCHEMA_SHA256_BYTES:
/// [u8; 32]` when interpolated with `#`. A doc comment written just before
/// the interpolation attaches to the first, the hex constant, as it always
/// has; the bytes constant carries its own.
pub(super) struct SchemaShaConsts {
    hex: String,
    bytes: [u8; 32],
}

impl SchemaShaConsts {
    /// `hex` is `parse::hash_schema_source`'s output: 64 lowercase hex
    /// digits. Anything else is a bug in this crate, so it panics at
    /// expansion time rather than emitting a wrong digest.
    pub(super) fn from_hex(hex: String) -> Self {
        assert_eq!(hex.len(), 64, "a SHA-256 is 64 hex digits");
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let pair = &hex[index * 2..index * 2 + 2];
            *byte = u8::from_str_radix(pair, 16).expect("hash_schema_source emits hex");
        }
        Self { hex, bytes }
    }
}

impl ToTokens for SchemaShaConsts {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let hex = &self.hex;
        let bytes = self.bytes.iter();
        tokens.extend(quote! {
            pub const SCHEMA_SHA256: &str = #hex;
            /// `SCHEMA_SHA256` as its 32 raw bytes: the `schema_sha` a COSE
            /// envelope binds into every signed message's AAD (ADR 0006 §4,
            /// cratestack#1006). It hashes the raw `.cstack` text, so a
            /// comment-only edit changes it (cratestack#1065).
            pub const SCHEMA_SHA256_BYTES: [u8; 32] = [#(#bytes),*];
        });
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaShaConsts;
    use crate::include::parse::hash_schema_source;

    #[test]
    fn the_bytes_are_the_hex_digest_decoded() {
        let hex = hash_schema_source("model Widget { id Int @id }");
        let consts = SchemaShaConsts::from_hex(hex.clone());
        let reencoded: String = consts.bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(reencoded, hex);
        assert_eq!(consts.bytes[0], 0x50);
        assert_eq!(consts.bytes[31], 0x3e);
    }

    #[test]
    fn both_constants_are_emitted() {
        let consts = SchemaShaConsts::from_hex(hash_schema_source(""));
        let emitted = quote::quote!(#consts).to_string();
        assert!(
            emitted.contains("pub const SCHEMA_SHA256 : & str"),
            "{emitted}"
        );
        assert!(
            emitted.contains("pub const SCHEMA_SHA256_BYTES : [u8 ; 32]"),
            "{emitted}"
        );
        // SHA-256 of the empty string starts e3 b0 c4 42.
        assert!(
            emitted.contains("[227u8 , 176u8 , 196u8 , 66u8"),
            "{emitted}"
        );
    }
}
