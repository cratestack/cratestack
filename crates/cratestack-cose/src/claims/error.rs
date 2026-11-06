//! Why a claim object was not signed or opened.

use cratestack_core::CratestackError;
use thiserror::Error;

use crate::error::UNAUTHENTICATED;

/// A failure of [`sign_claims`](crate::sign_claims) or
/// [`open_claims`](crate::open_claims).
///
/// The variants a verifier can reach carry nothing about the bytes it was
/// given, so a caller that forwards the error to the peer leaks nothing
/// beyond the variant (`Unverified` covers an unknown `kid` and a bad
/// signature alike). [`into_error`](Self::into_error) maps them onto the
/// envelope's `401` / `500` split.
///
/// `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClaimsError {
    /// Not a canonical, tagged `COSE_Sign1` of the claim-object profile: a
    /// wrong tag, a non-minimal or indefinite encoding, an unknown or
    /// out-of-order header label, a non-empty unprotected header, a detached
    /// payload, a missing `typ`, a MAC algorithm, trailing bytes, and so on.
    /// A transport envelope presented here ends up in this variant.
    #[error("not a canonical claim object")]
    Malformed,
    /// A well-formed claim object whose `typ` is not the one expected.
    #[error("claim object has an unexpected type")]
    WrongType,
    /// No key the resolver returned verified the signature. An unknown `kid`
    /// is this too: the two are deliberately not told apart.
    #[error("claim object signature did not verify")]
    Unverified,
    /// The key resolver failed (an outage, not a verdict on the object).
    #[error("claim key resolver unavailable")]
    Resolver(#[source] CratestackError),
    /// The signer failed (an outage, not a verdict on the inputs).
    #[error("claim signer failed")]
    Signer(#[source] CratestackError),
    /// The caller's inputs or signer are wrong, whatever the message: a MAC
    /// signer, a `kid` that is not 8 bytes, an empty or oversized `typ`,
    /// `aud` or `cti`, or a signature of the wrong length.
    #[error("claim object misuse: {0}")]
    Misuse(&'static str),
}

impl ClaimsError {
    /// The envelope's mapping (ADR 0006 §10): a verdict on the object is
    /// `Unauthorized` carrying the one fixed message, a backend failure or
    /// local misuse is `Internal`. A method, not a `From` impl: a second
    /// `From<_> for CratestackError` in a graph makes inference ambiguous
    /// (see `crate::error`).
    pub fn into_error(self) -> CratestackError {
        match self {
            Self::Malformed | Self::WrongType | Self::Unverified => {
                CratestackError::Unauthorized(UNAUTHENTICATED.to_owned())
            }
            other => CratestackError::Internal(format!("cose claims: {other}")),
        }
    }
}
