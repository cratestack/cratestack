//! The default fingerprint honours a verified principal (ADR 0006 §12,
//! cratestack#1006): a COSE-only client carries no `Authorization` header
//! and, served without `ConnectInfo`, used to be refused with a `412`.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};

use super::layer::default_principal_fingerprint;
use crate::ratelimit::VerifiedPrincipal;

fn request(principal: Option<&str>, authorization: Option<&str>) -> Request {
    let mut builder = http::Request::builder();
    if let Some(value) = authorization {
        builder = builder.header("authorization", value);
    }
    let mut req = Request::from(builder.body(Body::empty()).expect("request"));
    if let Some(principal) = principal {
        req.extensions_mut()
            .insert(VerifiedPrincipal(principal.to_owned()));
    }
    req
}

#[test]
fn a_verified_principal_is_namespaced_and_hashed() {
    let fingerprint =
        default_principal_fingerprint(&request(Some("cose:abcd"), None)).expect("no 412");
    // printf '%s' 'cose:abcd' | sha256sum
    assert_eq!(
        fingerprint,
        "princ:4ecd7e5a152ff95b08a3bd68a8057f200c67430b27bb7fbf09416439aa0ff097"
    );
    assert!(fingerprint.starts_with("princ:"));
    assert_eq!(fingerprint.len(), "princ:".len() + 64);
    assert!(!fingerprint.contains("cose:abcd"), "never stored verbatim");
}

#[test]
fn the_verified_principal_wins_over_authorization_and_the_peer() {
    let mut req = request(Some("cose:abcd"), Some("Bearer t"));
    let peer: SocketAddr = "192.0.2.1:1".parse().expect("addr");
    req.extensions_mut().insert(ConnectInfo(peer));
    let with_everything = default_principal_fingerprint(&req).expect("fingerprint");
    let principal_only =
        default_principal_fingerprint(&request(Some("cose:abcd"), None)).expect("fingerprint");
    assert_eq!(with_everything, principal_only);
}

#[test]
fn distinct_principals_get_distinct_namespaces() {
    let a = default_principal_fingerprint(&request(Some("cose:aa"), None)).expect("a");
    let b = default_principal_fingerprint(&request(Some("cose:bb"), None)).expect("b");
    assert_ne!(a, b);
}

#[test]
fn without_a_principal_the_old_refusal_stands() {
    let error = default_principal_fingerprint(&request(None, None)).expect_err("412");
    assert_eq!(error.status_code(), http::StatusCode::PRECONDITION_FAILED);
}
