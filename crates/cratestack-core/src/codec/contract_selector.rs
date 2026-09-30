//! The `Cratestack-Contract` selector header (cratestack#1123, EXT-14,
//! binding version 2).
//!
//! A signed message binds the **op contract digest** (32 bytes) into its
//! AAD, and the AAD is never sent, so a verifier that accepts more than one
//! digest for an op cannot tell which one the sender used. The sender says
//! so in this header: the first 8 bytes of that digest as unpadded
//! base64url (11 characters).
//!
//! The header is **not bound and not trusted**. It only picks which of the
//! digests the verifier *already accepts* for the op to bind; the AAD still
//! carries all 32 bytes, so a lie (or an attacker's rewrite of the header)
//! makes the verification fail with the ordinary `401`, never widens what
//! is accepted. A selector that names no accepted digest is answered with
//! an unsigned `426 contract_unsupported` before any key is looked up.
//!
//! Eight bytes is a selector, not an identity: two accepted digests of one
//! op sharing a prefix are both tried.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::CratestackError;

/// The request header that carries the selector.
pub const CONTRACT_HEADER: &str = "Cratestack-Contract";

/// The selector's length in bytes: a prefix of the op contract digest.
pub const CONTRACT_SELECTOR_LEN: usize = 8;

/// The header value's exact length: 8 bytes as unpadded base64url.
pub const CONTRACT_HEADER_VALUE_LEN: usize = 11;

/// The `RpcErrorBody` code (REST: `CONTRACT_UNSUPPORTED`) of the unsigned
/// `426` a verifier answers when the selector names no accepted digest.
pub const CONTRACT_UNSUPPORTED_CODE: &str = "contract_unsupported";

/// The first [`CONTRACT_SELECTOR_LEN`] bytes of an op contract digest.
///
/// ```
/// use cratestack_core::ContractSelector;
///
/// let digest = [0xab; 32];
/// let selector = ContractSelector::of(&digest);
/// assert_eq!(selector.to_header_value().len(), 11);
/// assert!(selector.matches(&digest));
/// assert!(!selector.matches(&[0xac; 32]));
/// let parsed = ContractSelector::from_header_value(selector.to_header_value().as_bytes());
/// assert_eq!(parsed.unwrap(), selector);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContractSelector([u8; CONTRACT_SELECTOR_LEN]);

impl ContractSelector {
    /// The selector of `digest`.
    pub fn of(digest: &[u8; 32]) -> Self {
        let mut prefix = [0; CONTRACT_SELECTOR_LEN];
        prefix.copy_from_slice(&digest[..CONTRACT_SELECTOR_LEN]);
        Self(prefix)
    }

    /// Whether `digest` starts with these bytes.
    pub fn matches(&self, digest: &[u8; 32]) -> bool {
        digest[..CONTRACT_SELECTOR_LEN] == self.0
    }

    /// The header value: 11 characters of unpadded base64url.
    pub fn to_header_value(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Parse a received header value, strictly: exactly 11 characters of
    /// the base64url alphabet, no padding, and the 2 unused trailing bits
    /// zero, so each selector has exactly one spelling.
    ///
    /// Fails with `CratestackError::BadRequest`, whose public message says
    /// only that the header is malformed.
    pub fn from_header_value(value: &[u8]) -> Result<Self, CratestackError> {
        let mut bytes = [0; CONTRACT_SELECTOR_LEN];
        if value.len() != CONTRACT_HEADER_VALUE_LEN {
            return Err(malformed());
        }
        match URL_SAFE_NO_PAD.decode_slice(value, &mut bytes) {
            Ok(CONTRACT_SELECTOR_LEN) => Ok(Self(bytes)),
            _ => Err(malformed()),
        }
    }
}

fn malformed() -> CratestackError {
    CratestackError::BadRequest(format!("malformed {CONTRACT_HEADER} header"))
}

impl fmt::Debug for ContractSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContractSelector({})", self.to_header_value())
    }
}
