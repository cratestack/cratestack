//! The two error shapes a verifier may produce (ADR 0006 §10, P0 default).

use cratestack_core::CratestackError;

/// The one message every failed verification check carries.
///
/// ADR 0006 §10: the response must never reveal which check failed. A
/// malformed body, an unknown `kid`, a disallowed algorithm, a tag that does
/// not match the algorithm, a bad signature, a stale `iat` and a replayed
/// `cti` all produce `CratestackError::Unauthorized(UNAUTHENTICATED.into())`,
/// byte for byte (`tests/oracle.rs` asserts it).
pub const UNAUTHENTICATED: &str = "request could not be authenticated";

/// A verification check failed. Deliberately carries nothing.
///
/// The parsing and verification stages return `Result<_, Reject>` rather
/// than `CratestackError`, so no stage *can* attach a reason: the only way
/// out is [`Reject::into_error`], which drops it. A diagnostic added in a
/// hurry during an incident therefore cannot leak to the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reject;

impl Reject {
    pub(crate) fn into_error(self) -> CratestackError {
        rejected()
    }
}

impl From<Reject> for CratestackError {
    fn from(reject: Reject) -> Self {
        reject.into_error()
    }
}

/// The coarse `401`.
pub(crate) fn rejected() -> CratestackError {
    CratestackError::Unauthorized(UNAUTHENTICATED.to_owned())
}

/// A backend the verifier depends on failed (the key resolver or the nonce
/// store). That says nothing about the message, so it is a `500`, and the
/// detail stays server-side: `CratestackError::Internal` keeps it out of
/// `public_message()`. An operator can then tell an outage from an attack.
pub(crate) fn backend(what: &str, error: CratestackError) -> CratestackError {
    CratestackError::Internal(format!("cose {what} unavailable: {error}"))
}

/// The local caller or its configuration is wrong (a binding of the wrong
/// shape, a signer whose algorithm does not fit the envelope, a `cti`
/// source that returns a malformed value). Also a `500`: it depends only on
/// local state, never on the received bytes, so it is not an oracle.
pub(crate) fn misuse(what: impl Into<String>) -> CratestackError {
    CratestackError::Internal(format!("cose envelope misuse: {}", what.into()))
}
