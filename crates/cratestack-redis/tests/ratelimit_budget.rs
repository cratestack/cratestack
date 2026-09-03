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

use std::time::Duration;

use cratestack_core::{
    BucketBudget, ConsumeRequest, RateLimitConfig, RateLimitDecision, RateLimitStore,
    bucket_ttl_secs, scope_ttl_secs,
};
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

/// **cratestack#871 review, blocker 2.** The scope record must OUTLIVE the
/// buckets it admitted. Measured before the fix with a window shorter than
/// the bucket TTL: every rollover minted a fresh epoch-suffixed key that
/// re-admitted `max_distinct` more buckets while the previous generation
/// was still alive — 21 buckets for a cap of 4 over five 1s windows.
///
/// Here the budget asks for a 1s window against a 5060s bucket TTL, and
/// drives 5x more distinct tokens than the cap over real time. The scope
/// TTL is raised to the bucket TTL, so there is no second generation.
#[tokio::test]
async fn a_scope_cannot_expire_underneath_the_buckets_it_admitted() {
    let Some((store, prefix, redis)) = store_or_skip("generations").await else {
        return;
    };
    const CAP: u32 = 4;
    // bucket_ttl_secs = ceil(5000 / 1.0) + 60 = 5060s.
    let config = RateLimitConfig::new(5000, 1.0);
    let budget = BucketBudget::new(
        "peer:198.51.100.9",
        "ip:198.51.100.9",
        CAP,
        Duration::from_secs(1),
    );

    for window in 0..5 {
        for nonce in 0..10 {
            let key = format!("auth:w{window}-{nonce}");
            store
                .consume_bounded(ConsumeRequest::new(&key, config, Some(&budget)))
                .await
                .expect("consume");
        }
        // Cross a real window boundary between generations.
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }

    let keys = scan_keys(&redis, &prefix).await;
    assert!(
        keys.len() <= CAP as usize + 2,
        "five 1s windows against a 5060s bucket TTL created {} keys; the bound is {} \
         ({CAP} buckets + 1 scope set + 1 fallback). Keys: {keys:?}",
        keys.len(),
        CAP as usize + 2,
    );
    assert_eq!(
        scopes(&keys).len(),
        1,
        "exactly one scope record, not one per window: {keys:?}",
    );
}

/// The scope record's TTL must be at least the bucket TTL — that is the
/// whole invariant blocker 2 turns on — and it must exist at all.
#[tokio::test]
async fn the_scope_set_ttl_covers_the_bucket_ttl() {
    let Some((store, prefix, redis)) = store_or_skip("ttl").await else {
        return;
    };
    let budget = budget(4);
    let config = RateLimitConfig::new(5, 0.001);
    store
        .consume_bounded(ConsumeRequest::new("auth:a", config, Some(&budget)))
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
    let expected = scope_ttl_secs(config, WINDOW) as i64 * 1000;
    assert!(
        pttl > 0 && pttl <= expected,
        "scope set PTTL was {pttl}ms; must be positive and at most {expected}ms",
    );
    // The invariant: the record outlives every bucket it admitted.
    let bucket_ttl_ms = bucket_ttl_secs(config) as i64 * 1000;
    assert!(
        pttl >= bucket_ttl_ms - 5_000,
        "scope PTTL {pttl}ms is shorter than the {bucket_ttl_ms}ms bucket TTL, so a second \
         generation can open underneath the buckets this one admitted",
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
