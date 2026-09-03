//! cratestack#871 at the LAYER level: the measured amplification attack,
//! run end-to-end through `RateLimitLayer` + `InMemoryRateLimitStore`.
//!
//! The attack, verbatim from the cratestack#846 security review: this
//! layer runs before authentication, so it hashes an unvalidated
//! `Authorization` header. A caller who rotates that header mints one
//! bucket per request. 20 requests produced 20 buckets; 500 would produce
//! 500 — see `without_bucket_budget_restores_the_amplification`, which
//! asserts exactly that and is the break-it proof for the test above it.

#![cfg(test)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use cratestack_core::{
    BucketBudget, Charged, ConsumeRequest, CratestackError, RateLimitConfig, RateLimitDecision,
};
use http::StatusCode;
use tower::{Layer, Service};

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::layer::RateLimitLayer;
use super::scope::KeyDerivation;
use super::store::{InMemoryRateLimitStore, RateLimitStore};
use super::tests_support::ok_service;

const PEER: &str = "192.0.2.77:41234";

/// One request in the rotation attack: same verified peer every time, a
/// brand-new bearer token every time.
fn rotating_request(nonce: usize) -> Request {
    let peer: SocketAddr = PEER.parse().expect("static addr");
    Request::builder()
        .header("authorization", format!("Bearer rotate-{nonce}"))
        .extension(ConnectInfo(peer))
        .body(Body::empty())
        .expect("request builds")
}

async fn drive(layer: RateLimitLayer, requests: usize) -> usize {
    let mut service = layer.layer(ok_service());
    let mut throttled = 0;
    for nonce in 0..requests {
        let response = service
            .call(rotating_request(nonce))
            .await
            .expect("infallible");
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            throttled += 1;
        }
    }
    throttled
}

/// Acceptance criterion 1, in-memory half: distinct buckets created is
/// bounded independently of N.
#[tokio::test]
async fn rotating_bearer_from_one_peer_cannot_mint_unbounded_buckets() {
    let store = Arc::new(InMemoryRateLimitStore::default());
    let layer = RateLimitLayer::new(
        store.clone(),
        // Slow refill so the fallback bucket's burst is the only budget
        // available inside the test window.
        RateLimitConfig::new(5, 0.001),
    )
    .with_bucket_budget(RateLimitBucketBudget::default().max_distinct_per_peer(8));

    let throttled = drive(layer, 500).await;

    // 8 admitted `auth:` buckets + the one `ip:` fallback they collapse
    // onto. NOT 500.
    assert!(
        store._bucket_count() <= 9,
        "500 rotating tokens created {} buckets; the budget bounds it at 9",
        store._bucket_count(),
    );
    // And the collapse is a throttle, not a bypass: once the peer's own
    // `ip:` bucket drains, the attacker is refused.
    assert!(
        throttled >= 400,
        "only {throttled} of 500 rotating requests were throttled; the fallback bucket is not \
         draining, which would make the collapse a free pass",
    );
}

/// The break-it proof for the test above, kept as a live assertion so it
/// cannot rot: remove the bound and the keyspace is caller-controlled
/// again, one bucket per request.
#[tokio::test]
async fn without_bucket_budget_restores_the_amplification() {
    let store = Arc::new(InMemoryRateLimitStore::default());
    let layer =
        RateLimitLayer::new(store.clone(), RateLimitConfig::new(5, 0.001)).without_bucket_budget();

    let throttled = drive(layer, 500).await;

    assert_eq!(
        store._bucket_count(),
        500,
        "without the budget, N requests must mint N buckets — if this ever stops holding, the \
         bound above is being proved by something other than the budget",
    );
    assert_eq!(
        throttled, 0,
        "and every one of them is allowed, because each request gets a full fresh burst",
    );
}

/// A store predating cratestack#871 implements only `consume`. It must
/// keep working unchanged — and the layer must say, once per hour rather
/// than once per request, that this deployment is not actually bounded.
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

#[tokio::test]
async fn a_store_without_consume_bounded_behaves_exactly_as_before() {
    let store = Arc::new(LegacyStore::default());
    let layer = RateLimitLayer::new(store.clone(), RateLimitConfig::new(5, 0.001));

    let throttled = drive(layer, 20).await;

    assert_eq!(throttled, 0);
    assert_eq!(
        store.calls.load(Ordering::Relaxed),
        20,
        "the default consume_bounded must delegate to consume, once per request",
    );

    // And it must SAY it did not apply the budget it was handed. A store
    // that reported `Requested` here would let a deployment believe it is
    // bounded when nothing bounded it — worse than being unbounded loudly.
    let budget = BucketBudget::new("peer:x", "ip:x", 1, Duration::from_secs(60));
    let outcome = store
        .consume_bounded(ConsumeRequest::new(
            "auth:whatever",
            RateLimitConfig::new(5, 0.001),
            Some(&budget),
        ))
        .await
        .expect("legacy store consumes");
    assert_eq!(outcome.charged, Charged::Unbounded);
}

#[test]
fn unbounded_store_warning_fires_once_and_only_when_a_budget_was_asked_for() {
    let warnings = Arc::new(BudgetWarnings::default());
    let budgeted = super::key_fn::default_key_fn(
        &rotating_request(0),
        RateLimitBucketBudget::default(),
        super::scope::UnverifiedAuthPolicy::default(),
        &warnings,
    )
    .expect("derives");
    let unbudgeted = KeyDerivation::unbudgeted("ip:192.0.2.1".to_owned());

    assert!(
        super::consume::report(cratestack_core::Charged::Unbounded, &budgeted, &warnings),
        "the first request against an unbounded store must warn",
    );
    assert!(
        !super::consume::report(cratestack_core::Charged::Unbounded, &budgeted, &warnings),
        "and the second must be throttled, or an attacker gets a log amplifier for free",
    );

    let quiet = Arc::new(BudgetWarnings::default());
    assert!(
        !super::consume::report(cratestack_core::Charged::Unbounded, &unbudgeted, &quiet),
        "a derivation that asked for no budget is not being let down by the store",
    );
}
