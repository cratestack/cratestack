//! Stable request fingerprint.

use http::Method;
use sha2::{Digest, Sha256};

/// Stable fingerprint of a request: SHA-256 over method, path + query,
/// content-type, and body bytes. Used to detect when a duplicate key is
/// reused with a different payload (the conflict case the draft spec
/// calls out). The `path` argument should include the query string so
/// modifier-style flags (`?dry_run=true`, `?confirm=true`) don't collide
/// — the middleware passes `Uri::path_and_query` for that reason.
pub fn hash_request(
    method: &Method,
    path: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(path.as_bytes());
    hasher.update(b"\0");
    hasher.update(content_type.unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(body);
    hasher.finalize().into()
}

/// Returns true if the HTTP method is one we'd guard with idempotency. We
/// apply only to mutating verbs — GETs are already safely repeatable.
pub fn is_idempotent_target_method(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PATCH | &Method::PUT | &Method::DELETE
    )
}
