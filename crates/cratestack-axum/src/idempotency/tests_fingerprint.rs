//! Tests for the default principal fingerprint derivation.

#![cfg(test)]

use axum::extract::Request;
use http::Request as HttpRequest;

use super::layer::default_principal_fingerprint;

#[test]
fn default_fingerprint_from_authorization_header() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let fingerprint = default_principal_fingerprint(&req);

    // Should hash the Authorization header value, not fall back to "anonymous".
    assert_ne!(fingerprint, "anonymous");
    // Should be consistent (same input → same output).
    let req2 = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = Request::from(req2);
    assert_eq!(fingerprint, default_principal_fingerprint(&req2));
}

#[test]
fn default_fingerprint_uses_client_ip_when_no_auth_header() {
    let req = HttpRequest::builder()
        .header("x-forwarded-for", "192.0.2.42")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let fingerprint = default_principal_fingerprint(&req);

    // Should use client IP instead of "anonymous".
    assert_eq!(fingerprint, "192.0.2.42");
}

#[test]
fn different_client_ips_produce_different_fingerprints() {
    let req1 = HttpRequest::builder()
        .header("x-forwarded-for", "192.0.2.1")
        .body(axum::body::Body::empty())
        .unwrap();
    let req1 = Request::from(req1);

    let req2 = HttpRequest::builder()
        .header("x-forwarded-for", "192.0.2.2")
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = Request::from(req2);

    let fp1 = default_principal_fingerprint(&req1);
    let fp2 = default_principal_fingerprint(&req2);

    // Two distinct clients without Authorization headers must produce
    // different fingerprints to avoid sharing an idempotency namespace.
    assert_ne!(fp1, fp2);
    assert_eq!(fp1, "192.0.2.1");
    assert_eq!(fp2, "192.0.2.2");
}

#[test]
fn authorization_header_takes_precedence_over_client_ip() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .header("x-forwarded-for", "192.0.2.42")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let fingerprint = default_principal_fingerprint(&req);

    // Authorization header should take precedence; IP should be ignored.
    assert_ne!(fingerprint, "192.0.2.42");
    assert_ne!(fingerprint, "anonymous");
}

#[test]
fn default_fingerprint_falls_back_to_anonymous_only_when_neither_auth_nor_ip_present() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let fingerprint = default_principal_fingerprint(&req);

    // Only as a last resort, when both Authorization and client IP are absent.
    assert_eq!(fingerprint, "anonymous");
}
