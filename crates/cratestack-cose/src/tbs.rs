//! The bytes a signature or MAC covers (RFC 9052 §4.4, §6.3):
//!
//! ```text
//! [ context: "Signature1" / "MAC0", protected: bstr, external_aad: bstr, payload: bstr ]
//! ```
//!
//! The structure is never assembled unless something needs it contiguous.
//! [`Tbs::with_chunks`] hands it over as nine slices (the fixed opening, the
//! `bstr` heads on the stack, and the three byte strings **where they
//! already are**), so every algorithm computes over it with no copy of the
//! payload, on the seal and on the open side (maintainer decisions on
//! cratestack#1005): HMAC and ESP256 hash it incrementally, and Ed25519
//! runs both PureEdDSA passes over the same slices. Only a signer that
//! wants the bytes themselves (a KMS, an HSM) gets [`Tbs::assemble`], which
//! copies the payload once.
//!
//! The protected header goes in **as received** (the caller passes the
//! wire slice), never re-encoded: RFC 9052 §4.4 signs the serialized bytes,
//! and re-encoding would make the result depend on the verifier's encoder.

use crate::alg::CoseMode;
use crate::cbor::write::{Head, MAJOR_BSTR, MAJOR_TSTR, len_arg};

/// A definite 4-element array head, the structure's first byte.
const ARRAY_OF_FOUR: [u8; 1] = [0x84];

#[derive(Debug, Clone, Copy)]
pub(crate) struct Tbs<'a> {
    pub(crate) mode: CoseMode,
    pub(crate) protected: &'a [u8],
    pub(crate) external_aad: &'a [u8],
    pub(crate) payload: &'a [u8],
}

impl Tbs<'_> {
    /// Call `f` with the structure as consecutive slices. Their
    /// concatenation is exactly [`Tbs::assemble`].
    pub(crate) fn with_chunks<R>(&self, f: impl FnOnce(&[&[u8]]) -> R) -> R {
        let context = self.mode.context();
        let context_head = Head::new(MAJOR_TSTR, len_arg(context.len()));
        let protected_head = Head::new(MAJOR_BSTR, len_arg(self.protected.len()));
        let aad_head = Head::new(MAJOR_BSTR, len_arg(self.external_aad.len()));
        let payload_head = Head::new(MAJOR_BSTR, len_arg(self.payload.len()));
        f(&[
            &ARRAY_OF_FOUR,
            context_head.as_slice(),
            context.as_bytes(),
            protected_head.as_slice(),
            self.protected,
            aad_head.as_slice(),
            self.external_aad,
            payload_head.as_slice(),
            self.payload,
        ])
    }

    /// The structure in one buffer, for a signer that takes the bytes (a
    /// KMS or an HSM). Sized exactly before anything is written.
    pub(crate) fn assemble(&self) -> Vec<u8> {
        self.with_chunks(|chunks| {
            let mut out = Vec::with_capacity(chunks.iter().map(|chunk| chunk.len()).sum());
            for chunk in chunks {
                out.extend_from_slice(chunk);
            }
            out
        })
    }
}
