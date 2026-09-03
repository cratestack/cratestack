//! The three evasions the cratestack#871 adversarial review measured, and
//! the one untested wiring it found. Each is a probe that produced a
//! *measured* number before the fix, quoted in its own doc comment.

#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use cratestack_core::{
    BucketBudget, ConsumeRequest, CratestackError, RateLimitConfig, RateLimitDecision,
};
use http::StatusCode;
use tower::{Layer, Service};

use super::budget::RateLimitBucketBudget;
use super::layer::RateLimitLayer;
use super::store::{InMemoryRateLimitStore, RateLimitStore};
use super::tests_support::ok_service;

/// One request from a chosen source address, optionally bearing a token.
pub(super) fn request(addr: &str, bearer: Option<usize>) -> Request {
    let mut builder = Request::builder().extension(ConnectInfo(
        addr.parse::<std::net::SocketAddr>().expect("addr parses"),
    ));
    if let Some(nonce) = bearer {
        builder = builder.header("authorization", format!("Bearer rotate-{nonce}"));
    }
    builder.body(Body::empty()).expect("request builds")
}

/// Walks 200 addresses through `2001:db8:1:2::/64` — one subscriber's
/// ordinary residential delegation.
fn address_in_one_64(nonce: usize) -> String {
    format!("[2001:db8:1:2::{nonce:x}]:1")
}

pub(super) async fn drive(
    layer: &RateLimitLayer,
    requests: impl Iterator<Item = Request>,
) -> usize {
    let mut service = layer.layer(ok_service());
    let mut allowed = 0;
    for req in requests {
        let response = service.call(req).await.expect("infallible");
        if response.status() != StatusCode::TOO_MANY_REQUESTS {
            allowed += 1;
        }
    }
    allowed
}

/// **Blocker 1a.** Measured before the fix: `Authorization` + a rotating
/// IPv6 source inside one /64, cap 8 → **200 buckets**. The /64 was
/// aggregated for the scope but not for the `ip:` fallback, so each
/// address got its own fallback bucket.
#[tokio::test]
async fn rotating_ipv6_inside_one_64_with_a_token_cannot_evade_the_cap() {
    let store = Arc::new(InMemoryRateLimitStore::default());
    let layer = RateLimitLayer::new(store.clone(), RateLimitConfig::new(5, 0.001))
        .with_bucket_budget(RateLimitBucketBudget::default().max_distinct_per_peer(8));

    drive(
        &layer,
        (0..200).map(|n| request(&address_in_one_64(n), Some(n))),
    )
    .await;

    assert!(
        store._bucket_count() <= 10,
        "rotating the SOURCE ADDRESS inside one /64 created {} buckets; the bound is 8 + 1 \
         fallback + 1 slack",
        store._bucket_count(),
    );
}

/// **Blocker 1b**, the worse half: with **no `Authorization` header at
/// all** there is no budget on this path, so aggregation is the only bound
/// there is. Measured before the fix: **200 buckets, 200/200 allowed** —
/// the cratestack#846 signature with the address as the rotating variable.
#[tokio::test]
async fn rotating_ipv6_inside_one_64_without_a_token_cannot_mint_buckets() {
    let store = Arc::new(InMemoryRateLimitStore::default());
    let layer = RateLimitLayer::new(store.clone(), RateLimitConfig::new(5, 0.001));

    let allowed = drive(
        &layer,
        (0..200).map(|n| request(&address_in_one_64(n), None)),
    )
    .await;

    assert_eq!(
        store._bucket_count(),
        1,
        "one /64 is one subscriber and must be one bucket",
    );
    assert!(
        allowed <= 5,
        "{allowed} of 200 requests were allowed; the burst is 5, so the /64 is not sharing a \
         bucket and the rotation is still free",
    );
}

/// **Blocker 2.** Measured before the fix: cap 4, window 1s, bucket TTL
/// 5060s → **21 buckets over 5 windows** (and 81 over 20), because the
/// scope record expired while the buckets it admitted were still alive and
/// the next generation re-admitted a fresh `max_distinct`.
///
/// The scope lifetime is now `scope_ttl_secs` — never below the bucket TTL
/// — so no second generation can open underneath the first.
#[test]
fn a_scope_cannot_expire_underneath_the_buckets_it_admitted() {
    const CAP: u32 = 4;
    let store = InMemoryRateLimitStore::default();
    // bucket_ttl_secs = ceil(5000/1.0) + 60 = 5060s, vastly longer than
    // the 1s window the budget asks for.
    let config = RateLimitConfig::new(5000, 1.0);
    let budget = BucketBudget::new("peer:p", "ip:p", CAP, Duration::from_secs(1));
    let start = Instant::now();

    for window in 0..5 {
        for nonce in 0..20 {
            let key = format!("auth:w{window}-{nonce}");
            store
                ._consume_at(
                    ConsumeRequest::new(&key, config, Some(&budget)),
                    start + Duration::from_secs(window),
                )
                .expect("consume");
        }
    }

    assert!(
        store._bucket_count() <= CAP as usize + 2,
        "five 1s windows against a 5060s bucket TTL created {} buckets; the bound is {}",
        store._bucket_count(),
        CAP as usize + 2,
    );
}

/// A store that only implements `consume`, so the layer must report
/// `Charged::Unbounded` and warn.
#[derive(Default)]
struct LegacyStore {
    calls: AtomicUsize,
}

#[async_trait]
impl RateLimitStore for LegacyStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(RateLimitDecision::Allowed { remaining: 1 })
    }
}

/// **Should-fix 3.** `consume::report(...)` could be deleted outright with
/// all 216 axum tests green: the warnings were only ever exercised by
/// calling `report` directly. This drives a real request through
/// `RateLimitLayer` and asserts the per-request path raised the condition.
#[tokio::test]
async fn the_layer_actually_reports_an_unbounded_store() {
    let layer = RateLimitLayer::new(
        Arc::new(LegacyStore::default()),
        RateLimitConfig::new(5, 0.001),
    );
    assert_eq!(layer._budget_warnings()._raised(), 0);

    drive(&layer, std::iter::once(request("192.0.2.1:1", Some(0)))).await;

    assert!(
        layer._budget_warnings()._raised() >= 1,
        "a budgeted derivation against a store that ignores the budget must raise a warning; \
         if this passes at 0, the report(...) call in consume.rs is dead",
    );
}

/// The counterpart: a request that asked for no budget is not being let
/// down by the store, so nothing should be raised.
#[tokio::test]
async fn no_warning_when_the_derivation_asked_for_no_budget() {
    let layer = RateLimitLayer::new(
        Arc::new(LegacyStore::default()),
        RateLimitConfig::new(5, 0.001),
    );

    // No Authorization header: the `ip:` key carries no budget.
    drive(&layer, std::iter::once(request("192.0.2.1:1", None))).await;

    assert_eq!(layer._budget_warnings()._raised(), 0);
}
