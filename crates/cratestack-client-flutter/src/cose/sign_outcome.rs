//! What a keystore signing callback ended with, as it crosses the bridge.
//!
//! An outcome is a *value* the Dart callback returns, never an exception it
//! throws: a `DartFnFuture` that completes with an error panics in the
//! generated glue, and `Result<_, String>` is not a bridgeable return type.
//! A plain struct and a C-like enum, not an enum with fields, for the reason
//! [`FlutterCoseError`](super::FlutterCoseError) is one: a data enum becomes
//! a `freezed` class.

/// How a signing call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterSignOutcomeKind {
    /// The callback produced a signature.
    Signed,
    /// The person dismissed the keystore's prompt.
    Cancelled,
    /// The keystore did not answer in time.
    TimedOut,
    /// The keystore failed, or the callback threw.
    Failed,
}

/// The result of one keystore signing call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterSignOutcome {
    /// How the call ended.
    pub kind: FlutterSignOutcomeKind,
    /// The ECDSA signature, DER encoded or raw `r ‖ s`. Empty unless
    /// `kind` is [`Signed`](FlutterSignOutcomeKind::Signed).
    pub signature: Vec<u8>,
    /// What went wrong. Read only for
    /// [`Failed`](FlutterSignOutcomeKind::Failed).
    pub message: String,
}

impl FlutterSignOutcome {
    /// A successful signature.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn signed(signature: Vec<u8>) -> Self {
        Self {
            kind: FlutterSignOutcomeKind::Signed,
            signature,
            message: String::new(),
        }
    }

    /// An outcome that is not a signature.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn of(kind: FlutterSignOutcomeKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            signature: Vec::new(),
            message: message.into(),
        }
    }
}
