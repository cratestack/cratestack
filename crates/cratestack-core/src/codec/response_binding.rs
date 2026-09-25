//! [`ResponseBinding`]: what a response's [`Binding`] adds to its
//! request's (ADR 0006 §4, as amended on 2026-09-25).
//!
//! [`Binding`]: super::Binding

/// How the request a response answers was sent, which fixes how its
/// digest was computed. The AAD encodes it right before the digest, as
/// [`code`](Self::code).
///
/// It exists because the two digest forms are not domain-separated:
/// `SHA-256(COSE bytes)` for a signed request and `SHA-256(nonce ‖ payload)`
/// for an unsigned one both hash bytes the client chose, so a signed
/// request `C` re-presented as an unsigned request with nonce `C[..16]` and
/// body `C[16..]` has the same digest. Before the kind was bound, the
/// server's response to that unsigned twin verified at the client as the
/// answer to `C` (maintainer decision on cratestack#1005, 2026-09-25).
///
/// Deliberately exhaustive: a new kind changes the AAD, so it is a new
/// binding version, and every encoder must be made to handle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKind {
    /// The request was not signed: its digest is SHA-256 over the client's
    /// 16-byte `Cratestack-Nonce` followed by the payload.
    Unsigned,
    /// The request was signed: its digest is SHA-256 over the request's
    /// COSE bytes exactly as they travelled.
    Signed,
}

impl RequestKind {
    /// The value the AAD carries: `0` for [`Unsigned`](Self::Unsigned), `1`
    /// for [`Signed`](Self::Signed).
    pub const fn code(self) -> u8 {
        match self {
            Self::Unsigned => 0,
            Self::Signed => 1,
        }
    }
}

/// A request's digest together with the [`RequestKind`] that says how it
/// was computed.
///
/// `cratestack-cose`'s `request_digest` and `request_digest_unsigned` return
/// this, so the digest and its kind come from one call and cannot disagree.
/// The fields are public, like [`Binding`](super::Binding)'s, only so that
/// a binding can be rebuilt verbatim from a vector file or another
/// implementation's output; filling them in by hand is how the two get
/// mismatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestDigest {
    /// How [`digest`](Self::digest) was computed.
    pub kind: RequestKind,
    /// SHA-256, as [`kind`](Self::kind) describes.
    pub digest: [u8; 32],
}

/// The part of a [`Binding`](super::Binding) only a response has: which
/// request it answers, and with what status.
///
/// One `Option` of this, instead of an `Option` per field, makes a binding
/// with a digest but no status (or a kind but no digest) unrepresentable,
/// so no encoder has to decide what such a third shape means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResponseBinding {
    /// The request this response answers.
    pub request: RequestDigest,
    /// The HTTP status code.
    pub status: u16,
}
