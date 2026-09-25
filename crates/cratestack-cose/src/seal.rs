//! Sealing: payload in, COSE message out, with the payload written once.
//!
//! ADR 0006 §1: "the sealer encodes straight into the COSE buffer: it
//! reserves up to 9 bytes for the `bstr` head, encodes, then patches the
//! head" (kept by the maintainer over accepting a copy, cratestack#1005).
//! The message is `prefix ‖ payload head ‖ payload ‖ signature`, where the
//! prefix (tag, array head, protected header, empty unprotected map) has a
//! length known before the payload exists, and the payload head's length
//! is known only after. So the buffer starts with `prefix_len + 9` bytes of
//! room, the payload is written right after that room, and once its length
//! is known the head (1 to 9 bytes) and then the prefix are written **right
//! aligned against the payload**. The `9 - head_len` bytes of room left at
//! the front are skipped by `Bytes::advance`, which moves a pointer.
//!
//! So nothing is shifted: the ADR's "shift once if the reserved head is too
//! long" is avoided by writing the short, fixed-length prefix last instead
//! of moving the payload. The cost is at most 8 unused bytes in front of
//! the message's allocation.
//!
//! What remains is ordinary buffer growth. The payload's size is unknown
//! until it is encoded, so the buffer starts with room for
//! [`PAYLOAD_CAPACITY_HINT`] bytes of it, and if the codec writes more, or
//! the signature does not fit after it, the `Vec` grows, which the
//! allocator may do by moving it. That is the same growth the codec's own
//! `Vec` would go through; what is gone is the second buffer and the copy
//! from it into the message.
//!
//! [`seal`] serves both entry points. `seal_value` hands it a closure that
//! runs `CratestackCodec::encode_into` on the buffer; `seal` (bytes already
//! encoded) one that copies them in, the one unavoidable copy for a caller
//! that encoded elsewhere. Both therefore produce the same bytes.
//!
//! The codec runs on a buffer the sealer owns, so the sealer checks the
//! one thing `CratestackCodec::encode_into` promises that it relies on:
//! that the codec appended and left the reserved room alone. A codec that
//! replaced or truncated the buffer is refused as misuse (a `500`) rather
//! than allowed to make the length arithmetic below underflow. The room is
//! filled with [`RESERVED`], not zeros, so that a codec that truncates and
//! then pushes zeros (CBOR `0`) back is seen too; while it was zeros, one
//! that swapped the last byte of the room for a zero sealed an empty
//! payload.
//!
//! The signature is computed over the to-be-signed structure without
//! building it (see `tbs.rs`) when the signer can take it in pieces (every
//! in-process signer: HMAC, ESP256 and Ed25519); otherwise (a KMS or HSM
//! signer, which keeps the default `sign_chunks`) the structure is built
//! once and handed to the signer.

use bytes::{Buf, Bytes};
use cratestack_core::{Binding, CratestackError};

use crate::aad;
use crate::alg::CoseAlg;
use crate::cbor::write::{self, EMPTY_MAP, Head, MAJOR_ARRAY, MAJOR_BSTR, MAJOR_TAG, MAX_HEAD_LEN};
use crate::envelope::Inner;
use crate::error::{backend, misuse};
use crate::header;
use crate::keys::esp256_low_s;
use crate::tbs::Tbs;

/// Seal the payload `write_payload` appends, for `bind`, as a request
/// (`request == true`, with `iat` and `cti`) or a response (`kid` and `alg`
/// only; Q1). `payload_hint` sizes the buffer; the payload may exceed it.
pub(crate) async fn seal<W>(
    inner: &Inner,
    bind: &Binding<'_>,
    request: bool,
    payload_hint: usize,
    write_payload: W,
) -> Result<Bytes, CratestackError>
where
    W: FnOnce(&mut Vec<u8>) -> Result<(), CratestackError>,
{
    if request == bind.response.is_some() {
        return Err(misuse(if request {
            "sealing a request with a response binding"
        } else {
            "sealing a response with a request binding"
        }));
    }
    let external_aad = aad::external_aad(bind)?;
    let signer = inner.signer.as_ref();
    let alg = signer.alg();
    let cti;
    let claims = if request {
        let iat = u64::try_from((inner.clock)())
            .map_err(|_| misuse("the clock returned a time before 1970"))?;
        cti = (inner.cti)()?;
        if !header::cti_len_ok(cti.len()) {
            return Err(misuse("the cti source must return 1 to 4 or 16 bytes"));
        }
        Some((iat, cti.as_slice()))
    } else {
        None
    };
    let protected = header::encode(alg, signer.kid(), claims);

    let tag = Head::new(MAJOR_TAG, inner.mode.tag());
    let array = Head::new(MAJOR_ARRAY, 4);
    let protected_head = Head::new(MAJOR_BSTR, write::len_arg(protected.len()));
    let prefix_len = tag.as_slice().len()
        + array.as_slice().len()
        + protected_head.as_slice().len()
        + protected.len()
        + 1;
    let payload_at = prefix_len + MAX_HEAD_LEN;
    let mut out =
        Vec::with_capacity(payload_at + payload_hint + write::bstr_len(alg.signature_len()));
    out.resize(payload_at, RESERVED);
    write_payload(&mut out)?;
    if out.len() < payload_at || out[..payload_at].iter().any(|&byte| byte != RESERVED) {
        return Err(misuse(
            "the codec's encode_into replaced or rewrote the buffer instead of appending to it",
        ));
    }

    let payload_head = Head::new(MAJOR_BSTR, write::len_arg(out.len() - payload_at));
    let start = MAX_HEAD_LEN - payload_head.as_slice().len();
    let mut at = start;
    for part in [
        tag.as_slice(),
        array.as_slice(),
        protected_head.as_slice(),
        &protected,
        &[EMPTY_MAP],
        payload_head.as_slice(),
    ] {
        out[at..at + part.len()].copy_from_slice(part);
        at += part.len();
    }
    debug_assert_eq!(at, payload_at);

    let tbs = Tbs {
        mode: inner.mode,
        protected: &protected,
        external_aad: &external_aad,
        payload: &out[payload_at..],
    };
    let signature = match tbs.with_chunks(|chunks| signer.sign_chunks(chunks)) {
        Some(signed) => signed,
        None => {
            let to_be_signed = tbs.assemble();
            signer.sign(&to_be_signed).await
        }
    }
    .map_err(|error| backend("signer", error))?;
    let signature = checked_signature(alg, signature)?;

    write::bstr(&mut out, &signature);
    let mut sealed = Bytes::from(out);
    sealed.advance(start);
    Ok(sealed)
}

/// Refuse a signature of the wrong length (Q5: sizes come from `alg`), and
/// normalise ESP256 to low-`s`, whichever signer produced it: a KMS or
/// WebCrypto signer may return either, and the opener accepts only low-`s`
/// (so no third party can re-spell a signed message; see
/// `CoseVerifyKey::verify`).
fn checked_signature(alg: CoseAlg, signature: Vec<u8>) -> Result<Vec<u8>, CratestackError> {
    if signature.len() != alg.signature_len() {
        return Err(misuse(
            "the signer returned a signature of the wrong length",
        ));
    }
    if alg == CoseAlg::Esp256 {
        return esp256_low_s(&signature)
            .ok_or_else(|| misuse("the signer returned an invalid ESP256 signature"));
    }
    Ok(signature)
}

/// The capacity reserved for a payload `seal_value` has not encoded yet.
/// Past it, the buffer grows the way a `Vec` the codec owned would have.
pub(crate) const PAYLOAD_CAPACITY_HINT: usize = 256;

/// What the room in front of the payload is filled with until the headers
/// are written into it; any other byte there after `encode_into` is a
/// codec that did not just append. `0xff` because a codec that truncates
/// and then encodes writes the first byte of its output into the room, and
/// that byte is never `0xff`: in CBOR it is the "break" stop code, never
/// the start of a data item (RFC 8949 §3.2.1), and it never occurs in the
/// UTF-8 of JSON text (RFC 3629). Every byte of the room is overwritten or
/// skipped (`Bytes::advance`) before the message is returned, so the
/// marker never reaches the wire, and a codec that appends never writes
/// there, so no valid encoder output can collide with it.
const RESERVED: u8 = 0xff;
