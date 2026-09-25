//! What a successful open returns.

use std::fmt;

use bytes::Bytes;

use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

/// A message that verified.
///
/// This is the typed result behind [`CratestackEnvelope::open`]: the axum
/// layer (cratestack#1006) runs before any `CratestackContext` exists, so it
/// needs the verified facts as values rather than as a side effect on a
/// context. `payload` and `cti` are zero-copy slices of the received body;
/// `kid` is a copy, so a caller that keeps only the signer's identity does
/// not keep the body alive.
///
/// `Debug` prints the payload's length, not its bytes: a verified body is
/// application data (a payment, a token), and `{:?}` on a result is how it
/// would otherwise end up in a log.
///
/// `#[non_exhaustive]` so the `auth` feature can add the device or service
/// id a resolver maps the key to without a breaking change.
///
/// [`CratestackEnvelope::open`]: cratestack_core::CratestackEnvelope::open
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Opened {
    /// The payload, exactly as signed, for `codec.decode`.
    pub payload: Bytes,
    /// The `kid` from the protected header. It is the verifying key's own
    /// `kid`: a candidate whose `kid` differs is never tried.
    pub kid: [u8; KID_LEN],
    /// The algorithm the message was verified with.
    pub alg: CoseAlg,
    /// The RFC 9679 thumbprint of the key that verified. Two keys can share
    /// a `kid` (8 bytes collide at about 2³² keys; `tests/replay.rs` uses a
    /// real pair), so this, not the `kid`, identifies the signer
    /// unambiguously; a principal derived from a verified message should use
    /// it. The trait path records it as the context's
    /// `VerifiedSigner::thumbprint`, the same name.
    pub thumbprint: [u8; 32],
    /// `iat`, seconds. `Some` for a request, `None` for a response.
    pub iat: Option<u64>,
    /// `cti`. `Some` for a request, `None` for a response.
    pub cti: Option<Bytes>,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opened")
            .field("payload_len", &self.payload.len())
            .field("kid", &self.kid)
            .field("alg", &self.alg)
            .field("thumbprint", &self.thumbprint)
            .field("iat", &self.iat)
            .field("cti", &self.cti)
            .finish()
    }
}
