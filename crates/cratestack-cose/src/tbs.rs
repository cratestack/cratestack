//! The bytes a signature or MAC covers (RFC 9052 §4.4, §6.3):
//!
//! ```text
//! [ context: "Signature1" / "MAC0", protected: bstr, external_aad: bstr, payload: bstr ]
//! ```
//!
//! The structure is never assembled unless something needs it contiguous.
//! [`Tbs::with_chunks`] hands it over as nine slices (the fixed opening, the
//! `bstr` heads on the stack, and the three byte strings **where they
//! already are**), so HMAC and ESP256 hash it incrementally, on the seal
//! and on the open side, with no copy of the payload (maintainer decision
//! on cratestack#1005). Ed25519 is the exception: PureEdDSA (RFC 8032) is
//! not a pre-hash scheme, and `ed25519-dalek`'s public signing and strict
//! verification APIs take the message as one slice, so for Ed25519 the
//! structure is built once with [`Tbs::assemble`], which copies the payload
//! once.
//!
//! The protected header goes in **as received** (the caller passes the
//! wire slice), never re-encoded: RFC 9052 §4.4 signs the serialized bytes,
//! and re-encoding would make the result depend on the verifier's encoder.

use std::cell::OnceCell;

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

    /// The structure in one buffer: for Ed25519, and for a signer that
    /// takes the bytes (a KMS). Sized exactly before anything is written.
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

/// A [`Tbs`] being verified against several candidate keys: the contiguous
/// form is built at most once, and only if an Ed25519 candidate asks.
pub(crate) struct TbsView<'a> {
    tbs: Tbs<'a>,
    contiguous: OnceCell<Vec<u8>>,
}

impl<'a> TbsView<'a> {
    pub(crate) fn new(tbs: Tbs<'a>) -> Self {
        Self {
            tbs,
            contiguous: OnceCell::new(),
        }
    }

    pub(crate) fn tbs(&self) -> &Tbs<'a> {
        &self.tbs
    }

    pub(crate) fn contiguous(&self) -> &[u8] {
        self.contiguous.get_or_init(|| self.tbs.assemble())
    }
}
