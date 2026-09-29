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
    /// `bytes` is `cratestack_core::schema_digest`'s output, the schema's
    /// canonical identity; the hex constant is derived from it here.
    pub(super) fn from_digest(bytes: [u8; 32]) -> Self {
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
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
            /// cratestack#1006). It is the schema's canonical identity, not
            /// a hash of its text: a comment or whitespace edit leaves it
            /// unchanged (cratestack#1065).
            pub const SCHEMA_SHA256_BYTES: [u8; 32] = [#(#bytes),*];
        });
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
