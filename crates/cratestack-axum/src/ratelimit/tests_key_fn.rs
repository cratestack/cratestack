//! Tests for the default rate-limit key function.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::layer::default_key_fn;

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

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
fn default_key_uses_connect_info_when_no_auth_header() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = with_connect_info(Request::from(req), "192.0.2.42:12345");

    let key = default_key_fn(&req);

    // Should use the verified peer address with "ip:" prefix instead of
    // "anonymous".
    assert_eq!(key, "ip:192.0.2.42");
}

#[test]
fn different_connect_info_addrs_produce_different_rate_limit_keys() {
    let req1 = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req1 = with_connect_info(Request::from(req1), "192.0.2.1:1");

    let req2 = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = with_connect_info(Request::from(req2), "192.0.2.2:1");

    let key1 = default_key_fn(&req1);
    let key2 = default_key_fn(&req2);

    // Two distinct peers without Authorization headers must produce
    // different rate-limit keys to avoid sharing a rate-limit bucket.
    assert_ne!(key1, key2);
    assert_eq!(key1, "ip:192.0.2.1");
    assert_eq!(key2, "ip:192.0.2.2");
}

#[test]
fn authorization_header_takes_precedence_over_connect_info() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = with_connect_info(Request::from(req), "192.0.2.42:1");

    let key = default_key_fn(&req);

    // Authorization header should take precedence; peer address should be
    // ignored.
    assert!(key.starts_with("auth:"));
    assert_ne!(key, "ip:192.0.2.42");
    assert_ne!(key, "anonymous");
}

#[test]
fn default_key_falls_back_to_anonymous_when_no_connect_info_extension() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let key = default_key_fn(&req);

    // Only as a last resort, when neither Authorization nor a verified peer
    // address (via the `ConnectInfo` extension) is present.
    assert_eq!(key, "anonymous");
}

#[test]
fn spoofed_forwarded_headers_are_ignored_without_connect_info() {
    // Regression test: `Forwarded`/`X-Forwarded-For` are client-supplied and
    // must never be trusted as a rate-limit key on their own -- this crate
    // has no trusted-proxy configuration to verify or strip them. Without a
    // `ConnectInfo` extension (i.e. no verified peer address), two requests
    // that spoof distinct `X-Forwarded-For` values must still collapse onto
    // the same shared "anonymous" bucket rather than getting a fresh bucket
    // per spoofed value.
    let req1 = HttpRequest::builder()
        .header("x-forwarded-for", "203.0.113.1")
        .body(axum::body::Body::empty())
        .unwrap();
    let req1 = Request::from(req1);

    let req2 = HttpRequest::builder()
        .header("x-forwarded-for", "203.0.113.2")
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = Request::from(req2);

    let key1 = default_key_fn(&req1);
    let key2 = default_key_fn(&req2);

    assert_eq!(key1, "anonymous");
    assert_eq!(key2, "anonymous");
    assert_eq!(key1, key2);
}
