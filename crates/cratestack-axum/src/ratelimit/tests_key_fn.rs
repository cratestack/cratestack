//! Tests for the default rate-limit key function.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::key_fn::default_key_fn;
use super::scope::{BudgetScope, KeyDerivation, UnverifiedAuthPolicy};

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

/// The default configuration, which is what every pre-cratestack#871
/// assertion below is still asserting against.
fn derive(req: &Request) -> Result<KeyDerivation, cratestack_core::CratestackError> {
    default_key_fn(
        req,
        RateLimitBucketBudget::default(),
        UnverifiedAuthPolicy::default(),
        &BudgetWarnings::default(),
    )
}

fn bearer(token: &str) -> Request {
    Request::from(
        HttpRequest::builder()
            .header("authorization", token)
            .body(axum::body::Body::empty())
            .unwrap(),
    )
}

fn bare() -> Request {
    Request::from(
        HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap(),
    )
}

#[test]
fn default_key_from_authorization_header() {
    let derivation = derive(&bearer("Bearer token123")).expect("authorization header present");

    // Should hash the Authorization header value with "auth:" prefix.
    assert!(derivation.key.starts_with("auth:"));
    assert_ne!(derivation.key, "anonymous");
    // Should be consistent (same input → same output).
    let again = derive(&bearer("Bearer token123")).expect("authorization header present");
    assert_eq!(derivation.key, again.key);
}

#[test]
fn default_key_uses_connect_info_when_no_auth_header() {
    let req = with_connect_info(bare(), "192.0.2.42:12345");

    let derivation = derive(&req).expect("ConnectInfo present");

    // Should use the verified peer address with "ip:" prefix instead of
    // "anonymous".
    assert_eq!(derivation.key, "ip:192.0.2.42");
    // A verified peer address is not caller-mintable, so it needs no
    // cardinality budget (cratestack#871).
    assert!(derivation.budget.is_none());
}

#[test]
fn different_connect_info_addrs_produce_different_rate_limit_keys() {
    let key1 = derive(&with_connect_info(bare(), "192.0.2.1:1"))
        .expect("ConnectInfo present")
        .key;
    let key2 = derive(&with_connect_info(bare(), "192.0.2.2:1"))
        .expect("ConnectInfo present")
        .key;

    // Two distinct peers without Authorization headers must produce
    // different rate-limit keys to avoid sharing a rate-limit bucket.
    assert_ne!(key1, key2);
    assert_eq!(key1, "ip:192.0.2.1");
    assert_eq!(key2, "ip:192.0.2.2");
}

#[test]
fn authorization_header_takes_precedence_over_connect_info() {
    let req = with_connect_info(bearer("Bearer token123"), "192.0.2.42:1");

    let derivation = derive(&req).expect("authorization header present");

    // Authorization header should take precedence; peer address should be
    // ignored for the KEY, and used for the budget SCOPE instead.
    assert!(derivation.key.starts_with("auth:"));
    assert_ne!(derivation.key, "ip:192.0.2.42");
    assert_ne!(derivation.key, "anonymous");
    let budget = derivation.budget.expect("unverified auth must be budgeted");
    assert_eq!(budget.scope_key, "peer:192.0.2.42");
    assert_eq!(budget.fallback_key, "ip:192.0.2.42");
    assert_eq!(
        budget.max_distinct,
        RateLimitBucketBudget::DEFAULT_MAX_DISTINCT_PER_PEER
    );
    assert_eq!(derivation.scope, Some(BudgetScope::Peer));
}

/// cratestack#416: the pre-existing default silently fell back to a shared
/// `"anonymous"` string here. There is no unforgeable value left to key on
/// once both Authorization and ConnectInfo are absent, so the default must
/// now refuse the request instead of manufacturing a shared bucket.
#[test]
fn default_key_refuses_when_no_connect_info_extension() {
    let error = derive(&bare())
        .expect_err("neither Authorization nor ConnectInfo present must not succeed");
    assert_eq!(error.status_code(), http::StatusCode::PRECONDITION_FAILED);
}

#[test]
fn spoofed_forwarded_headers_are_ignored_without_connect_info() {
    // Regression test: `Forwarded`/`X-Forwarded-For` are client-supplied and
    // must never be trusted as a rate-limit key on their own -- this crate
    // has no trusted-proxy configuration to verify or strip them. Without a
    // `ConnectInfo` extension (i.e. no verified peer address), two requests
    // that spoof distinct `X-Forwarded-For` values must both be refused
    // identically (cratestack#416: no longer a shared "anonymous" bucket,
    // but still never a bucket keyed off the attacker-controlled header
    // value).
    let req1 = Request::from(
        HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.1")
            .body(axum::body::Body::empty())
            .unwrap(),
    );
    let req2 = Request::from(
        HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.2")
            .body(axum::body::Body::empty())
            .unwrap(),
    );

    assert!(
        derive(&req1).is_err(),
        "spoofable header must not be trusted"
    );
    assert!(
        derive(&req2).is_err(),
        "spoofable header must not be trusted"
    );
}
