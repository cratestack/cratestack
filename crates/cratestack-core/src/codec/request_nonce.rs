//! The client nonce that binds a signed response to one **unsigned**
//! request (maintainer decision on cratestack#1005, 2026-09-24).
//!
//! A response is bound to its request through `request_digest` (§4). For a
//! signed request that is a digest of the COSE message, whose `cti` makes
//! it unique. An unsigned request has no such thing: every `GET` of one URL
//! has the same (empty) body, so without a nonce a signed response to one
//! `GET` verifies as the answer to every later `GET` of that URL, and a
//! cache or a terminating hop could serve a stale one forever. The client
//! therefore sends 16 fresh random bytes in a `Cratestack-Nonce` header on
//! every request it wants a verifiable response to (in `Required` mode, all
//! of them), and for an unsigned request
//!
//! ```text
//! request_digest = SHA-256(nonce (16 bytes) ‖ payload)   ; payload empty for a GET
//! ```
//!
//! A signed request keeps `request_digest = SHA-256(COSE bytes)`. The two
//! forms are not domain-separated (a signed request's bytes `C` hash like
//! an unsigned request with nonce `C[..16]` and payload `C[16..]`), so the
//! response AAD also binds which one it is, as `request_kind` (2026-09-25).
//! Both helpers return a `RequestDigest` that carries its kind.
//!
//! This module provides the pieces only. Sending the header (the Rust
//! client, cratestack#1007) and reading it before building the response
//! binding (the axum layer, cratestack#1006) are wired there.
//!
//! Moved here from `cratestack-cose` (which re-exports every item under its
//! old path) by the maintainer's decision after the cratestack#1006 API
//! review: the axum layer's `envelope` feature computes these digests for
//! any envelope, and must not pull a COSE or signature crate to do it.
//!
//! Parsing and formatting only. **Drawing a random nonce is not here**
//! (second-review decision B-1, 2026-09-26): it needs `getrandom`, whose
//! `wasm32-unknown-unknown` build fails unless someone selects its
//! `wasm_js` backend, and core is compiled for that target by crates that
//! never draw a nonce (`cratestack-cbor-wasm`, `cratestack-sqlite`). The
//! one caller, the client side of `cratestack-cose`, has
//! `cratestack_cose::random_request_nonce` and
//! `CoseEnvelope::request_nonce`, and selects the backend itself.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use super::response_binding::{RequestDigest, RequestKind};
use crate::error::CratestackError;

/// The request header that carries the nonce.
pub const NONCE_HEADER: &str = "Cratestack-Nonce";

/// Nonce length in bytes.
pub const REQUEST_NONCE_LEN: usize = 16;

/// The header value's exact length: 16 bytes as unpadded base64url.
pub const NONCE_HEADER_VALUE_LEN: usize = 22;

/// A `Cratestack-Nonce`: 16 bytes, chosen by the client.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestNonce([u8; REQUEST_NONCE_LEN]);

impl RequestNonce {
    /// A nonce with the given bytes: for tests, vectors, and a client with
    /// its own randomness (16 bytes from a CSPRNG, fresh per request).
    /// Anything else should use `cratestack_cose::random_request_nonce` or
    /// `cratestack_cose::CoseEnvelope::request_nonce` (see the module docs
    /// for why there is no `random` here).
    pub const fn from_bytes(bytes: [u8; REQUEST_NONCE_LEN]) -> Self {
        Self(bytes)
    }

    /// The nonce's bytes.
    pub fn as_bytes(&self) -> &[u8; REQUEST_NONCE_LEN] {
        &self.0
    }

    /// The header value: 22 characters of unpadded base64url.
    pub fn to_header_value(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Parse a received header value, strictly: exactly 22 characters of
    /// the base64url alphabet, no padding, and the 4 unused trailing bits
    /// zero, so each nonce has exactly one spelling.
    ///
    /// Fails with `CratestackError::BadRequest` (whose message is public and
    /// says only that the header is malformed). The envelope layer
    /// (cratestack#1006) treats a missing or malformed nonce as no nonce: the
    /// unsigned request runs, and its response is not sealed.
    pub fn from_header_value(value: &[u8]) -> Result<Self, CratestackError> {
        let mut bytes = [0; REQUEST_NONCE_LEN];
        if value.len() != NONCE_HEADER_VALUE_LEN {
            return Err(malformed());
        }
        match URL_SAFE_NO_PAD.decode_slice(value, &mut bytes) {
            Ok(REQUEST_NONCE_LEN) => Ok(Self(bytes)),
            _ => Err(malformed()),
        }
    }

    /// [`from_header_value`](Self::from_header_value) for a header that
    /// may be absent: `None` is `CratestackError::BadRequest` too.
    pub fn from_header(value: Option<&[u8]>) -> Result<Self, CratestackError> {
        match value {
            Some(value) => Self::from_header_value(value),
            None => Err(CratestackError::BadRequest(format!(
                "missing {NONCE_HEADER} header"
            ))),
        }
    }
}

fn malformed() -> CratestackError {
    CratestackError::BadRequest(format!("malformed {NONCE_HEADER} header"))
}

impl fmt::Debug for RequestNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RequestNonce({})", self.to_header_value())
    }
}

/// The request digest of a response to an **unsigned** request: SHA-256
/// over the 16 nonce bytes followed by the request payload exactly as
/// received (empty for a bodiless request), marked
/// [`RequestKind::Unsigned`]. The nonce has a fixed length, so the split
/// between the two is unambiguous.
pub fn request_digest_unsigned(nonce: &RequestNonce, payload: &[u8]) -> RequestDigest {
    let mut digest = Sha256::new();
    digest.update(nonce.0);
    digest.update(payload);
    RequestDigest {
        kind: RequestKind::Unsigned,
        digest: digest.finalize().into(),
    }
}

/// The request digest a response binding carries when the request was
/// **signed**: SHA-256 over the request body exactly as it travelled, the
/// whole COSE message (tag, headers, signature and all), marked
/// [`RequestKind::Signed`]. The caller passes the received body, never a
/// re-encoding of it.
///
/// For an unsigned request, use [`request_digest_unsigned`], which also
/// binds the client's `Cratestack-Nonce`.
pub fn request_digest(signed_request_body: &[u8]) -> RequestDigest {
    RequestDigest {
        kind: RequestKind::Signed,
        digest: Sha256::digest(signed_request_body).into(),
    }
}
