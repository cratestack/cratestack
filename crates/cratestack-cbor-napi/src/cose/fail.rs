//! The error every COSE call throws: `{ code, message }`, the shape the web
//! build rejects with (`crates/cratestack-cbor-wasm/src/cose/error.rs`).

use cratestack_core::CratestackError;

/// `code` of a failed verification: no detail, by design (ADR 0006 §10), so
/// the Node client is no more of an oracle than the server.
pub(crate) const REJECTED: &str = "rejected";
/// `code` of local misuse: a binding of the wrong shape, a bad key, a signer
/// that failed. It depends on local state only, never on received bytes.
pub(crate) const MISUSE: &str = "misuse";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fail {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl Fail {
    pub(crate) fn misuse(message: impl Into<String>) -> Fail {
        Fail {
            code: MISUSE,
            message: message.into(),
        }
    }
}

/// The one mapping from the Rust errors, so no call site decides what a
/// caller sees.
impl From<CratestackError> for Fail {
    fn from(error: CratestackError) -> Fail {
        match error {
            CratestackError::Unauthorized(_) => Fail {
                code: REJECTED,
                message: String::new(),
            },
            CratestackError::Internal(message) | CratestackError::Validation(message) => {
                Fail::misuse(message)
            }
            other => Fail::misuse(other.to_string()),
        }
    }
}
