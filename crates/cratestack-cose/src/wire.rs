//! The outer message and the to-be-signed structure (RFC 9052 §4.2, §4.4,
//! §6.2, §6.3), emitted and parsed by hand. See [`crate::cbor`] for why.
//!
//! ```text
//! Tag(18 | 17) [ protected: bstr, unprotected: {}, payload: bstr, signature/tag: bstr ]
//! ```
//!
//! The parser accepts only that shape: the expected tag, a definite
//! 4-element array, a non-empty protected `bstr`, an unprotected header
//! that is exactly `0xa0`, an embedded payload (a detached `nil` payload is
//! rejected) and no trailing bytes. The unprotected check is the one that
//! matters most: nothing in it is authenticated, so any content there is
//! something an attacker could have added (§3: "nothing unauthenticated on
//! the wire").

use std::ops::Range;

use crate::alg::CoseMode;
use crate::cbor::read::Reader;
use crate::cbor::write::{self, EMPTY_MAP, MAJOR_ARRAY, MAJOR_TAG};
use crate::error::Reject;

/// Where each part of a parsed message sits in the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) protected: Range<usize>,
    pub(crate) payload: Range<usize>,
    pub(crate) signature: Range<usize>,
}

/// Emit a tagged message. The buffer is sized exactly, and the payload is
/// copied into it once.
pub(crate) fn emit(mode: CoseMode, protected: &[u8], payload: &[u8], signature: &[u8]) -> Vec<u8> {
    let len = write::head_len(mode.tag())
        + 1
        + write::bstr_len(protected.len())
        + 1
        + write::bstr_len(payload.len())
        + write::bstr_len(signature.len());
    let mut out = Vec::with_capacity(len);
    write::head(&mut out, MAJOR_TAG, mode.tag());
    write::head(&mut out, MAJOR_ARRAY, 4);
    write::bstr(&mut out, protected);
    out.push(EMPTY_MAP);
    write::bstr(&mut out, payload);
    write::bstr(&mut out, signature);
    debug_assert_eq!(out.len(), len);
    out
}

/// Parse a tagged message of the given mode, strictly.
pub(crate) fn parse(mode: CoseMode, body: &[u8]) -> Result<Parts, Reject> {
    let mut reader = Reader::new(body);
    if reader.expect(MAJOR_TAG)? != mode.tag() {
        return Err(Reject);
    }
    if reader.expect(MAJOR_ARRAY)? != 4 {
        return Err(Reject);
    }
    let protected = reader.bstr_range()?;
    if protected.is_empty() {
        return Err(Reject);
    }
    reader.expect_byte(EMPTY_MAP)?;
    let payload = reader.bstr_range()?;
    let signature = reader.bstr_range()?;
    if !reader.is_at_end() {
        return Err(Reject);
    }
    Ok(Parts {
        protected,
        payload,
        signature,
    })
}

/// The bytes a signature or MAC covers:
/// `[context, protected, external_aad, payload]`, with the three byte
/// fields as `bstr`s. The protected header goes in **as received** (the
/// caller passes the wire slice), never re-encoded: RFC 9052 §4.4 signs the
/// serialized bytes, and re-encoding would make the verifier's result
/// depend on the verifier's encoder.
///
/// The payload is copied here a second time, which the `CoseSigner`
/// interface (`sign(&[u8])`, so a KMS can take the bytes) makes
/// unavoidable. Streaming the structure into the hash would avoid it for
/// HMAC and ESP256, but not for Ed25519, which is not a pre-hash scheme.
pub(crate) fn to_be_signed(
    mode: CoseMode,
    protected: &[u8],
    external_aad: &[u8],
    payload: &[u8],
) -> Vec<u8> {
    let context = mode.context();
    let len = 1
        + write::head_len(write::len_arg(context.len()))
        + context.len()
        + write::bstr_len(protected.len())
        + write::bstr_len(external_aad.len())
        + write::bstr_len(payload.len());
    let mut out = Vec::with_capacity(len);
    write::head(&mut out, MAJOR_ARRAY, 4);
    write::tstr(&mut out, context);
    write::bstr(&mut out, protected);
    write::bstr(&mut out, external_aad);
    write::bstr(&mut out, payload);
    debug_assert_eq!(out.len(), len);
    out
}
