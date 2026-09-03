//! cratestack#871: which SCOPE the default key function puts a request
//! in, and what budget comes with it. The key-shape assertions the same
//! function has always had live next door in `tests_key_fn.rs`.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::key_fn::default_key_fn;
use super::scope::{BudgetScope, KeyDerivation, UnverifiedAuthPolicy, VerifiedPrincipal};

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

fn derive(req: &Request) -> Result<KeyDerivation, cratestack_core::CratestackError> {
    default_key_fn(
        req,
        RateLimitBucketBudget::default(),
        UnverifiedAuthPolicy::default(),
        &BudgetWarnings::default(),
    )
}

fn derive_with(req: &Request, policy: UnverifiedAuthPolicy) -> KeyDerivation {
    default_key_fn(
        req,
        RateLimitBucketBudget::default(),
        policy,
        &BudgetWarnings::default(),
    )
    .expect("derivation must succeed")
}

fn bearer(token: &str) -> Request {
    Request::from(
        HttpRequest::builder()
            .header("authorization", token)
            .body(axum::body::Body::empty())
            .unwrap(),
    )
}

#[test]
fn verified_principal_keys_on_princ_and_carries_no_budget() {
    let mut req = bearer("Bearer attacker-rotates-this");
    req.extensions_mut()
        .insert(VerifiedPrincipal("user-42".to_owned()));
    let req = with_connect_info(req, "192.0.2.7:1");

    let derivation = derive(&req).expect("verified principal present");

    assert!(
        derivation.key.starts_with("princ:"),
        "key was {}",
        derivation.key
    );
    // The principal must be hashed, never keyed verbatim.
    assert!(!derivation.key.contains("user-42"));
    assert!(
        derivation.budget.is_none(),
        "a verified principal is not caller-mintable, so it needs no cap",
    );
}

#[test]
fn ignore_policy_keys_on_the_peer_despite_an_authorization_header() {
    let req = with_connect_info(bearer("Bearer token123"), "192.0.2.42:1");

    let derivation = derive_with(&req, UnverifiedAuthPolicy::Ignore);

    assert_eq!(derivation.key, "ip:192.0.2.42");
    assert!(derivation.budget.is_none());
}

/// The unconfigured-proxy case: an `Authorization` header with nothing to
/// attribute its cardinality to. The key shape is unchanged from before
/// cratestack#871 (so no existing bucket moves), but it is now counted
/// against one process-global budget.
#[test]
fn authorization_without_a_peer_falls_into_the_global_scope() {
    let derivation = derive(&bearer("Bearer token123")).expect("authorization header present");

    assert!(derivation.key.starts_with("auth:"));
    let budget = derivation.budget.expect("must be budgeted");
    assert_eq!(budget.scope_key, "global");
    assert_eq!(budget.fallback_key, "overflow");
    assert_eq!(
        budget.max_distinct,
        RateLimitBucketBudget::DEFAULT_MAX_DISTINCT_GLOBAL
    );
    assert_eq!(derivation.scope, Some(BudgetScope::Global));
}

/// Without /64 aggregation the per-peer cap is free to evade: one ordinary
/// residential IPv6 delegation is 2^64 addresses, i.e. 2^64 scopes AND
/// 2^64 fallback buckets.
#[test]
fn ipv6_scopes_and_fallbacks_both_aggregate_to_a_64() {
    let one = derive(&with_connect_info(
        bearer("Bearer a"),
        "[2001:db8:1:2:3:4:5:6]:1",
    ))
    .expect("derives");
    let two = derive(&with_connect_info(
        bearer("Bearer b"),
        "[2001:db8:1:2:aaaa:bbbb:cccc:dddd]:1",
    ))
    .expect("derives");
    let other_64 = derive(&with_connect_info(
        bearer("Bearer c"),
        "[2001:db8:1:3::1]:1",
    ))
    .expect("derives");

    let scope = |d: &KeyDerivation| d.budget.as_ref().expect("budgeted").scope_key.clone();
    assert_eq!(scope(&one), "peer:2001:db8:1:2::/64");
    assert_eq!(
        scope(&one),
        scope(&two),
        "two addresses in one /64 must share a cardinality scope",
    );
    assert_ne!(
        scope(&one),
        scope(&other_64),
        "a different /64 is a different scope",
    );

    // The FALLBACK is aggregated too (cratestack#871 review, blocker 1).
    // A /64-scoped budget whose fallback is per-address hands an attacker
    // 2^64 free fallback buckets — the bound evaded from the other side,
    // measured at 200 buckets for a cap of 8. The accepted cost is that
    // two hosts in one /64 share a throttling bucket.
    let fallback = |d: &KeyDerivation| d.budget.as_ref().expect("budgeted").fallback_key.clone();
    assert_eq!(fallback(&one), "ip:2001:db8:1:2::/64");
    assert_eq!(fallback(&one), fallback(&two));
    assert_ne!(fallback(&one), fallback(&other_64));
}

/// The path with NO budget at all, and therefore the one where
/// aggregation is the only bound there is: rotating the source address
/// inside one /64 with no `Authorization` header minted one bucket per
/// request, all allowed (cratestack#871 review, blocker 1).
#[test]
fn ipv6_unauthenticated_keys_aggregate_to_a_64() {
    let bare = || {
        Request::from(
            HttpRequest::builder()
                .body(axum::body::Body::empty())
                .unwrap(),
        )
    };
    let one = derive(&with_connect_info(bare(), "[2001:db8:9:9:1:2:3:4]:1")).expect("derives");
    let two =
        derive(&with_connect_info(bare(), "[2001:db8:9:9:dead:beef:0:1]:1")).expect("derives");
    let other = derive(&with_connect_info(bare(), "[2001:db8:9:a::1]:1")).expect("derives");

    assert_eq!(one.key, "ip:2001:db8:9:9::/64");
    assert_eq!(
        one.key, two.key,
        "one /64 is one bucket, or the cap is free"
    );
    assert_ne!(one.key, other.key, "a different /64 is a different bucket");
    assert!(one.budget.is_none());
}
