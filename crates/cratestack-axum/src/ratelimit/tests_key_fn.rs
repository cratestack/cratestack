//! Tests for the default rate-limit key function.

#![cfg(test)]

use axum::extract::Request;
use http::Request as HttpRequest;

use super::layer::default_key_fn;

#[test]
fn default_key_from_authorization_header() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let key = default_key_fn(&req);

    // Should hash the Authorization header value with "auth:" prefix.
    assert!(key.starts_with("auth:"));
    assert_ne!(key, "anonymous");
    // Should be consistent (same input → same output).
    let req2 = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = Request::from(req2);
    assert_eq!(key, default_key_fn(&req2));
}

#[test]
fn default_key_uses_client_ip_when_no_auth_header() {
    let req = HttpRequest::builder()
        .header("x-forwarded-for", "192.0.2.42")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let key = default_key_fn(&req);

    // Should use client IP with "ip:" prefix instead of "anonymous".
    assert_eq!(key, "ip:192.0.2.42");
}

#[test]
fn different_client_ips_produce_different_rate_limit_keys() {
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

    let key1 = default_key_fn(&req1);
    let key2 = default_key_fn(&req2);

    // Two distinct clients without Authorization headers must produce
    // different rate-limit keys to avoid sharing a rate-limit bucket.
    assert_ne!(key1, key2);
    assert_eq!(key1, "ip:192.0.2.1");
    assert_eq!(key2, "ip:192.0.2.2");
}

#[test]
fn authorization_header_takes_precedence_over_client_ip() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .header("x-forwarded-for", "192.0.2.42")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let key = default_key_fn(&req);

    // Authorization header should take precedence; IP should be ignored.
    assert!(key.starts_with("auth:"));
    assert_ne!(key, "ip:192.0.2.42");
    assert_ne!(key, "anonymous");
}

#[test]
fn default_key_falls_back_to_anonymous_only_when_neither_auth_nor_ip_present() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let key = default_key_fn(&req);

    // Only as a last resort, when both Authorization and client IP are absent.
    assert_eq!(key, "anonymous");
}
