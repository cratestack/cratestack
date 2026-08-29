//! Pure canonicalisation-function tests: no key material, no I/O.

use http::Method;

use crate::signed_request::{canonical_query, canonical_signature_base, content_sha256_base64url};

#[test]
fn canonicalizes_query_keys_lexicographically() {
    assert_eq!(canonical_query(Some("z=9&a=1&a=2&b=3")), "a=1&a=2&b=3&z=9");
}

#[test]
fn content_hash_uses_base64url_sha256() {
    assert_eq!(
        content_sha256_base64url(b"hello"),
        "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ"
    );
}

#[test]
fn canonical_signature_base_uses_newline_join() {
    assert_eq!(
        canonical_signature_base(
            &Method::POST,
            "/uploads/presign",
            Some("b=2&a=1"),
            "hash",
            "2026-04-24T12:00:00Z",
            "n_123",
            "vk_123"
        ),
        "POST\n/uploads/presign\na=1&b=2\nhash\n2026-04-24T12:00:00Z\nn_123\nvk_123"
    );
}
