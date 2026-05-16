//! Request-hash tests.

#![cfg(test)]

use http::Method;

use super::hash::{hash_request, is_idempotent_target_method};

#[test]
fn hash_changes_with_body() {
    let a = hash_request(&Method::POST, "/transfer", Some("application/cbor"), b"{}");
    let b = hash_request(
        &Method::POST,
        "/transfer",
        Some("application/cbor"),
        b"{\"x\":1}",
    );
    assert_ne!(a, b);
}

#[test]
fn hash_changes_with_query_string() {
    // Same method, same body, same content-type, different query —
    // must hash differently. Pre-fix the middleware fed only
    // `Uri::path` into the hasher and `?dry_run=true` collided
    // with `?dry_run=false`.
    let a = hash_request(
        &Method::POST,
        "/transfer?dry_run=true",
        Some("application/json"),
        b"{}",
    );
    let b = hash_request(
        &Method::POST,
        "/transfer?dry_run=false",
        Some("application/json"),
        b"{}",
    );
    assert_ne!(a, b);
}

#[test]
fn hash_changes_with_method_or_path() {
    let a = hash_request(&Method::POST, "/transfer", None, b"payload");
    let b = hash_request(&Method::PATCH, "/transfer", None, b"payload");
    let c = hash_request(&Method::POST, "/credit", None, b"payload");
    assert_ne!(a, b);
    assert_ne!(a, c);
}

#[test]
fn idempotent_target_method_predicate_excludes_get() {
    assert!(!is_idempotent_target_method(&Method::GET));
    assert!(!is_idempotent_target_method(&Method::HEAD));
    assert!(is_idempotent_target_method(&Method::POST));
    assert!(is_idempotent_target_method(&Method::PATCH));
    assert!(is_idempotent_target_method(&Method::PUT));
    assert!(is_idempotent_target_method(&Method::DELETE));
}
