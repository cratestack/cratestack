//! cratestack#871 review, blocker 1: IPv6 `/64` aggregation on the paths
//! that are NOT the budgeted `auth:` scope — the `Ignore` policy's peer
//! key. The scope/fallback aggregation lives next door in
//! `tests_scope.rs`; this file exists because the two together exceed the
//! workspace's 200-line ceiling.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::key_fn::default_key_fn;
use super::scope::{KeyDerivation, UnverifiedAuthPolicy};
use cratestack_core::CratestackError;

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

fn derive(req: &Request) -> Result<KeyDerivation, CratestackError> {
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

/// `Ignore` drops the header and keys on the peer — which for IPv6 must
/// also be the /64, or the policy advertised as "strictly stronger" is
/// weaker than the default it replaces.
#[test]
fn ipv6_ignore_policy_keys_on_the_aggregated_peer() {
    let one = derive_with(
        &with_connect_info(bearer("Bearer a"), "[2001:db8:5:5::1]:1"),
        UnverifiedAuthPolicy::Ignore,
    );
    let two = derive_with(
        &with_connect_info(bearer("Bearer b"), "[2001:db8:5:5:ffff::2]:1"),
        UnverifiedAuthPolicy::Ignore,
    );

    assert_eq!(one.key, "ip:2001:db8:5:5::/64");
    assert_eq!(one.key, two.key);
}

/// IPv4 is deliberately not aggregated: a /24 under CGNAT is thousands of
/// unrelated subscribers, and IPv4 gives an attacker no free-address supply
/// comparable to a /64.
#[test]
fn ipv4_scopes_are_not_aggregated() {
    let one = derive(&with_connect_info(bearer("Bearer a"), "192.0.2.1:1")).expect("derives");
    let two = derive(&with_connect_info(bearer("Bearer b"), "192.0.2.2:1")).expect("derives");

    let scope = |d: &KeyDerivation| d.budget.as_ref().expect("budgeted").scope_key.clone();
    assert_eq!(scope(&one), "peer:192.0.2.1");
    assert_ne!(scope(&one), scope(&two));
}
