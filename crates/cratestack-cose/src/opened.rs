//! What a successful open returns.

use bytes::Bytes;

use crate::alg::CoseAlg;

/// A message that verified.
///
/// This is the typed result behind [`CratestackEnvelope::open`]: the axum
/// layer (cratestack#1006) runs before any `CratestackContext` exists, so it
/// needs the verified facts as values rather than as a side effect on a
/// context. Every `Bytes` field is a zero-copy slice of the received body.
///
/// `#[non_exhaustive]` so the `auth` feature can add the device or service
/// id a resolver maps the key to without a breaking change.
///
/// [`CratestackEnvelope::open`]: cratestack_core::CratestackEnvelope::open
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Opened {
    /// The payload, exactly as signed, for `codec.decode`.
    pub payload: Bytes,
    /// The `kid` from the protected header (8 bytes). It is the verifying
    /// key's own `kid`: a candidate whose `kid` differs is never tried.
    pub kid: Bytes,
    /// The algorithm the message was verified with.
    pub alg: CoseAlg,
    /// The RFC 9679 thumbprint of the key that verified. Two keys can share
    /// a `kid` (8 bytes collide at about 2³² keys; `tests/replay.rs` uses a
    /// real pair), so this, not the `kid`, identifies the signer
    /// unambiguously; a principal derived from a verified message should use
    /// it. The trait path records it in the context's `VerifiedSigner`.
    pub key_thumbprint: [u8; 32],
    /// `iat`, seconds. `Some` for a request, `None` for a response.
    pub iat: Option<u64>,
    /// `cti`. `Some` for a request, `None` for a response.
    pub cti: Option<Bytes>,
}
