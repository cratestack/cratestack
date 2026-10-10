//! [`FlutterCoseError`]: the one error a sealed call surfaces to Dart.

use std::fmt;

use cratestack_core::CratestackError;

/// Why sealing or opening failed.
///
/// [`Rejected`](Self::Rejected) is every failed verification check. The
/// Dart client must not become an oracle for which check failed any more
/// than the server is (ADR 0006 §10), so a rejection carries no message at
/// all. The other kinds depend on local state only (a binding of the wrong
/// shape, a signer that failed), never on the received bytes, so they may
/// say what went wrong.
///
/// The three `Signer*` kinds are produced by a keystore-backed signer,
/// which a later release adds; the in-memory signers of this release cannot
/// fail that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterCoseErrorKind {
    /// The message did not verify (the coarse `401`).
    Rejected,
    /// The local caller or configuration is wrong.
    Misuse,
    /// The person dismissed the keystore's prompt.
    SignerCancelled,
    /// The keystore did not answer in time.
    SignerTimedOut,
    /// The keystore failed.
    SignerFailed,
}

/// A failed seal or open: a [`FlutterCoseErrorKind`] and, for
/// [`Misuse`](FlutterCoseErrorKind::Misuse) and
/// [`SignerFailed`](FlutterCoseErrorKind::SignerFailed), a message.
///
/// A struct and not an enum with data because `flutter_rust_bridge` turns
/// an enum with fields into a `freezed` class, which would make every
/// consuming app run `build_runner`. Dart sees it as the sealed
/// `CoseException` hierarchy of `package:cratestack_cbor/cose.dart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterCoseError {
    /// What went wrong.
    pub kind: FlutterCoseErrorKind,
    /// Empty for every kind but `Misuse` and `SignerFailed`.
    pub message: String,
}

impl FlutterCoseError {
    /// The coarse rejection: no detail, by design.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn rejected() -> Self {
        Self::of(FlutterCoseErrorKind::Rejected, String::new())
    }

    /// Local misuse, with what was wrong.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn misuse(message: impl Into<String>) -> Self {
        Self::of(FlutterCoseErrorKind::Misuse, message.into())
    }

    fn of(kind: FlutterCoseErrorKind, message: String) -> Self {
        Self { kind, message }
    }
}

/// The single mapping from the Rust errors, so no call site can decide
/// differently what a caller sees.
impl From<CratestackError> for FlutterCoseError {
    fn from(error: CratestackError) -> Self {
        match error {
            CratestackError::Unauthorized(_) => Self::rejected(),
            CratestackError::Internal(message) | CratestackError::Validation(message) => {
                Self::misuse(message)
            }
            other => Self::misuse(other.to_string()),
        }
    }
}

impl fmt::Display for FlutterCoseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            FlutterCoseErrorKind::Rejected => f.write_str("rejected"),
            FlutterCoseErrorKind::Misuse => write!(f, "misuse: {}", self.message),
            FlutterCoseErrorKind::SignerCancelled => f.write_str("the signer was cancelled"),
            FlutterCoseErrorKind::SignerTimedOut => f.write_str("the signer timed out"),
            FlutterCoseErrorKind::SignerFailed => {
                write!(f, "the signer failed: {}", self.message)
            }
        }
    }
}

impl std::error::Error for FlutterCoseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unauthorized_is_the_same_detail_free_rejection() {
        let a = FlutterCoseError::from(CratestackError::Unauthorized("one".to_owned()));
        let b = FlutterCoseError::from(CratestackError::Unauthorized("two".to_owned()));
        assert_eq!(a, FlutterCoseError::rejected());
        assert_eq!(a, b);
        assert_eq!(a.message, "");
        assert_eq!(a.to_string(), "rejected");
    }

    #[test]
    fn local_failures_are_misuse_with_their_message() {
        for error in [
            CratestackError::Internal("bad binding".to_owned()),
            CratestackError::Validation("bad binding".to_owned()),
        ] {
            assert_eq!(
                FlutterCoseError::from(error),
                FlutterCoseError::misuse("bad binding")
            );
        }
    }
}
