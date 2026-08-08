//! Tests for the default principal fingerprint derivation.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::layer::default_principal_fingerprint;

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

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
fn default_fingerprint_uses_connect_info_when_no_auth_header() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = with_connect_info(Request::from(req), "192.0.2.42:12345");

    let fingerprint = default_principal_fingerprint(&req);

    // Should use the verified peer address instead of "anonymous".
    assert_eq!(fingerprint, "192.0.2.42");
}

#[test]
fn different_connect_info_addrs_produce_different_fingerprints() {
    let req1 = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req1 = with_connect_info(Request::from(req1), "192.0.2.1:1");

    let req2 = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req2 = with_connect_info(Request::from(req2), "192.0.2.2:1");

    let fp1 = default_principal_fingerprint(&req1);
    let fp2 = default_principal_fingerprint(&req2);

    // Two distinct peers without Authorization headers must produce
    // different fingerprints to avoid sharing an idempotency namespace.
    assert_ne!(fp1, fp2);
    assert_eq!(fp1, "192.0.2.1");
    assert_eq!(fp2, "192.0.2.2");
}

#[test]
fn authorization_header_takes_precedence_over_connect_info() {
    let req = HttpRequest::builder()
        .header("authorization", "Bearer token123")
        .body(axum::body::Body::empty())
        .unwrap();
    let req = with_connect_info(Request::from(req), "192.0.2.42:1");

    let fingerprint = default_principal_fingerprint(&req);

    // Authorization header should take precedence; peer address should be
    // ignored.
    assert_ne!(fingerprint, "192.0.2.42");
    assert_ne!(fingerprint, "anonymous");
}

#[test]
fn default_fingerprint_falls_back_to_anonymous_when_no_connect_info_extension() {
    let req = HttpRequest::builder()
        .body(axum::body::Body::empty())
        .unwrap();
    let req = Request::from(req);

    let fingerprint = default_principal_fingerprint(&req);

    // Only as a last resort, when neither Authorization nor a verified peer
    // address (via the `ConnectInfo` extension) is present.
    assert_eq!(fingerprint, "anonymous");
}

#[test]
fn spoofed_forwarded_headers_do_not_let_an_attacker_pick_another_callers_namespace() {
    // Regression test: `Forwarded`/`X-Forwarded-For` are client-supplied and
    // must never be trusted as an idempotency namespace on their own -- this
    // crate has no trusted-proxy configuration to verify or strip them. An
    // attacker who sets `X-Forwarded-For` to a value they believe another
    // caller uses must not land in that caller's namespace. Without a
    // `ConnectInfo` extension (i.e. no verified peer address), both a
    // "victim" IP and an attacker-spoofed identical header must collapse
    // onto the same shared "anonymous" namespace, not a namespace keyed off
    // the attacker-controlled header value.
    let victim_req = HttpRequest::builder()
        .header("x-forwarded-for", "203.0.113.9")
        .body(axum::body::Body::empty())
        .unwrap();
    let victim_req = Request::from(victim_req);

    let attacker_req = HttpRequest::builder()
        .header("x-forwarded-for", "203.0.113.9")
        .body(axum::body::Body::empty())
        .unwrap();
    let attacker_req = Request::from(attacker_req);

    let victim_fp = default_principal_fingerprint(&victim_req);
    let attacker_fp = default_principal_fingerprint(&attacker_req);

    assert_eq!(victim_fp, "anonymous");
    assert_eq!(attacker_fp, "anonymous");
    // Not the spoofed header value -- that would mean the header was trusted.
    assert_ne!(victim_fp, "203.0.113.9");
}
