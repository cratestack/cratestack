//! Canonicalisation of the request onto the exact byte string that gets
//! signed / verified, and the signing helper built on top of it.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::Signer;
use http::Method;
use sha2::{Digest, Sha256};

use super::types::SignRequestParams;

/// Moved to `cratestack-core` (cratestack#1006) so the COSE envelope layer
/// binds the same bytes without depending on this crate; re-exported here so
/// `cratestack_auth::canonical_query` keeps working.
pub use cratestack_core::canonical_query;

pub fn content_sha256_base64url(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn canonical_signature_base(
    method: &Method,
    path: &str,
    query: Option<&str>,
    content_sha256: &str,
    timestamp: &str,
    nonce: &str,
    key_id: &str,
) -> String {
    [
        method.as_str().to_ascii_uppercase(),
        path.to_string(),
        canonical_query(query),
        content_sha256.to_string(),
        timestamp.to_string(),
        nonce.to_string(),
        key_id.to_string(),
    ]
    .join("\n")
}

pub fn sign_request(params: SignRequestParams<'_>) -> String {
    let signature_base = canonical_signature_base(
        params.method,
        params.path,
        params.query,
        &content_sha256_base64url(params.body),
        params.timestamp,
        params.nonce,
        params.key_id,
    );
    URL_SAFE_NO_PAD.encode(
        params
            .signing_key
            .sign(signature_base.as_bytes())
            .to_bytes(),
    )
}
