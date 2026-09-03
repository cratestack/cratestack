//! cratestack#871 against a REAL Redis: the measured amplification attack
//! from the cratestack#846 security review, and the bound that closes it.
//!
//! The attack, quoted from that review: `RateLimitLayer` runs before
//! authentication, so `default_key_fn` hashes an unvalidated
//! `Authorization` header. 20 requests with a rotating bearer produced
//! 20/20 allowed and **20 distinct Redis keys**, each with a ≥60s TTL;
//! driving that to `maxmemory` made every `HSET` fail. This file asserts
//! the key count is now bounded by the budget instead of by N.
//!
//! Break-it proof (run manually, quoted in the PR): delete the
//! `elseif card < max_distinct` branch's `else` arm in
//! `ratelimit/scripts.rs` — i.e. never charge the fallback — and
//! `rotating_bearer_cannot_mint_a_key_per_request` fails with 201 keys
//! instead of ≤10.
//!
//! Skipped unless `CRATESTACK_REDIS_TEST_URL` is set or
//! `CRATESTACK_USE_TESTCONTAINERS=1` (with `CRATESTACK_REQUIRE_REDIS=1` in
//! CI so a skip fails loudly rather than printing `ok`).

mod support;

use cratestack_core::{ConsumeRequest, RateLimitConfig, RateLimitDecision, RateLimitStore};
use redis::AsyncCommands;

use support::ratelimit_budget::{WINDOW, budget, scan_keys, scopes, store_or_skip};

/// Acceptance criterion 1: distinct buckets created is bounded
/// independently of N, asserted against a real Redis.
#[tokio::test]
async fn rotating_bearer_cannot_mint_a_key_per_request() {
    let Some((store, prefix, redis)) = store_or_skip("rotation").await else {
        return;
    };
    const CAP: u32 = 8;
    const REQUESTS: usize = 200;
    let budget = budget(CAP);
    let config = RateLimitConfig::new(5, 0.001);

    let mut throttled = 0;
    for nonce in 0..REQUESTS {
        let key = format!("auth:rotate-{nonce}");
        let outcome = store
            .consume_bounded(ConsumeRequest::new(&key, config, Some(&budget)))
            .await
            .expect("consume");
        if matches!(outcome.decision, RateLimitDecision::Throttled { .. }) {
            throttled += 1;
        }
    }

    let keys = scan_keys(&redis, &prefix).await;
    assert!(
        keys.len() <= CAP as usize + 2,
        "{REQUESTS} rotating tokens created {} Redis keys; the bound is {} ({CAP} buckets + 1 \
         scope set + 1 fallback bucket). Keys: {keys:?}",
        keys.len(),
        CAP as usize + 2,
    );
    assert!(
        throttled > 0,
        "the collapse onto the fallback must throttle, not wave traffic through",
    );
}

/// The scope set must not outlive its window, or the bound leaks one set
/// per window forever.
#[tokio::test]
async fn the_scope_set_carries_a_window_ttl() {
    let Some((store, prefix, redis)) = store_or_skip("ttl").await else {
        return;
    };
    let budget = budget(4);
    store
        .consume_bounded(ConsumeRequest::new(
            "auth:a",
            RateLimitConfig::new(5, 0.001),
            Some(&budget),
        ))
        .await
        .expect("consume");

    let keys = scan_keys(&redis, &prefix).await;
    let scope_keys = scopes(&keys);
    assert_eq!(scope_keys.len(), 1, "expected one scope set: {keys:?}");

    let mut conn = redis
        .client
        .get_multiplexed_async_connection()
        .await
        .expect("connect");
    let pttl: i64 = conn.pttl(scope_keys[0]).await.expect("pttl");
    assert!(
        pttl > 0 && pttl <= WINDOW.as_millis() as i64,
        "scope set PTTL was {pttl}ms; must be positive and at most the {}ms window",
        WINDOW.as_millis(),
    );
}

/// `consume` (no budget) must behave exactly as it did before
/// cratestack#871 — one key per caller, no scope set at all.
#[tokio::test]
async fn unbudgeted_consume_creates_no_scope_key() {
    let Some((store, prefix, redis)) = store_or_skip("unbudgeted").await else {
        return;
    };
    let config = RateLimitConfig::new(3, 0.001);
    for i in 0..5 {
        store
            .consume(&format!("k{i}"), config)
            .await
            .expect("consume");
    }

    let keys = scan_keys(&redis, &prefix).await;
    assert_eq!(keys.len(), 5, "five keys, one per caller: {keys:?}");
    assert!(
        scopes(&keys).is_empty(),
        "an unbudgeted consume must not create a scope set: {keys:?}",
    );
}
