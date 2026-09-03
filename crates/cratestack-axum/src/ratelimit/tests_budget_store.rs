//! cratestack#871 at the STORE level: distinct-bucket accounting, TTL
//! eviction, and the hard cap — all on an injected clock, because every
//! one of them is a clock decision and a test that sleeps through a real
//! 1060-second TTL is a test nobody runs.

#![cfg(test)]

use std::time::{Duration, Instant};

use cratestack_core::{
    BucketBudget, Charged, ConsumeRequest, CratestackError, RateLimitConfig, RateLimitDecision,
    bucket_ttl_secs,
};

use super::policy::StoreErrorPolicy;
use super::store::InMemoryRateLimitStore;

const CONFIG: RateLimitConfig = RateLimitConfig {
    burst: 5,
    refill_per_second: 0.001,
};

fn budget(max_distinct: u32) -> BucketBudget {
    BucketBudget::new(
        "peer:192.0.2.1",
        "ip:192.0.2.1",
        max_distinct,
        Duration::from_secs(60),
    )
}

fn consume(
    store: &InMemoryRateLimitStore,
    key: &str,
    budget: Option<&BucketBudget>,
    now: Instant,
) -> (RateLimitDecision, Charged) {
    let outcome = store
        ._consume_at(ConsumeRequest::new(key, CONFIG, budget), now)
        .expect("consume must succeed");
    (outcome.decision, outcome.charged)
}

/// cratestack#416 under cratestack#871: callers *within* the budget keep
/// their own buckets, and one exhausting its burst must not touch another's.
#[test]
fn distinct_callers_under_the_budget_never_share_a_bucket() {
    let store = InMemoryRateLimitStore::default();
    let budget = budget(8);
    let now = Instant::now();

    for _ in 0..3 {
        for user in 0..8 {
            let (decision, charged) =
                consume(&store, &format!("auth:user{user}"), Some(&budget), now);
            assert_eq!(
                charged,
                Charged::Requested,
                "user{user} must get its own bucket"
            );
            assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
        }
    }
    assert_eq!(store._bucket_count(), 8, "eight callers, eight buckets");

    // user0 spends the rest of its burst and goes over.
    for _ in 0..2 {
        consume(&store, "auth:user0", Some(&budget), now);
    }
    let (decision, _) = consume(&store, "auth:user0", Some(&budget), now);
    assert!(
        matches!(decision, RateLimitDecision::Throttled { .. }),
        "user0 must be throttled after spending its burst, got {decision:?}",
    );

    // user7 is untouched by that: 5 burst - 4 consumed = 1 left.
    let (decision, charged) = consume(&store, "auth:user7", Some(&budget), now);
    assert_eq!(charged, Charged::Requested);
    assert_eq!(decision, RateLimitDecision::Allowed { remaining: 1 });
}

/// Break-it proof for the test above: with the budget at 2, the same eight
/// callers DO share, and the isolation assertion above would fail.
#[test]
fn callers_beyond_the_budget_collapse_onto_the_fallback() {
    let store = InMemoryRateLimitStore::default();
    let budget = budget(2);
    let now = Instant::now();

    let charged: Vec<Charged> = (0..8)
        .map(|user| consume(&store, &format!("auth:user{user}"), Some(&budget), now).1)
        .collect();

    assert_eq!(&charged[..2], &[Charged::Requested, Charged::Requested]);
    assert!(
        charged[2..].iter().all(|c| *c == Charged::Fallback),
        "every caller past the cap must take the fallback: {charged:?}",
    );
    // 2 admitted buckets + 1 fallback.
    assert_eq!(store._bucket_count(), 3);
}

/// Acceptance criterion 2: the map does not grow monotonically.
#[test]
fn idle_buckets_are_evicted_once_a_full_ttl_has_passed() {
    let store = InMemoryRateLimitStore::default();
    let t0 = Instant::now();

    for i in 0..1000 {
        consume(&store, &format!("k{i}"), None, t0);
    }
    assert_eq!(store._bucket_count(), 1000, "all 1000 are live at t0");

    let ttl = Duration::from_secs(bucket_ttl_secs(CONFIG));
    consume(&store, "fresh", None, t0 + ttl + Duration::from_secs(1));

    assert_eq!(
        store._bucket_count(),
        1,
        "every bucket idle for a full TTL must be gone, leaving only the one just written",
    );
}

/// A sweep that runs while a bucket is still inside its TTL must leave it
/// alone: evicting early would hand a throttled caller a fresh burst, i.e.
/// turn the eviction fix into a limiter bypass.
///
/// 61s is past the sweep interval (so a sweep definitely runs) and far
/// inside the 1060s TTL this config derives, and refills only 0.061 of a
/// token — so a surviving bucket is still empty and a re-created one would
/// answer `Allowed { remaining: 4 }`.
#[test]
fn a_sweep_inside_the_ttl_does_not_reset_a_drained_bucket() {
    let store = InMemoryRateLimitStore::default();
    let t0 = Instant::now();

    for _ in 0..5 {
        consume(&store, "hot", None, t0);
    }
    let (decision, _) = consume(&store, "hot", None, t0 + Duration::from_secs(61));

    assert!(
        matches!(decision, RateLimitDecision::Throttled { .. }),
        "a bucket used inside the TTL must keep its state, got {decision:?}",
    );
    assert_eq!(store._bucket_count(), 1);
}

#[test]
fn max_buckets_fails_closed_when_a_sweep_frees_nothing() {
    let store = InMemoryRateLimitStore::default().with_max_buckets(10);
    let t0 = Instant::now();

    for i in 0..10 {
        consume(&store, &format!("k{i}"), None, t0);
    }

    let error = store
        ._consume_at(ConsumeRequest::new("k10", CONFIG, None), t0)
        .expect_err("the 11th distinct key must be refused");
    assert!(
        matches!(error, CratestackError::Internal(_)),
        "the cap is a LOGICAL failure, not a transport one: {error:?}",
    );
    assert!(
        !StoreErrorPolicy::Allow.permits(&error),
        "if `Allow` served this through, filling the map would be a limiter bypass",
    );

    // Buckets that already exist keep being served — the cap refuses only
    // the marginal new identity.
    let (decision, _) = consume(&store, "k0", None, t0);
    assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
}

/// The forced sweep at the cap: a burst that has since aged out must not
/// permanently wedge the store.
#[test]
fn the_cap_recovers_once_the_old_buckets_age_out() {
    let store = InMemoryRateLimitStore::default().with_max_buckets(10);
    let t0 = Instant::now();
    for i in 0..10 {
        consume(&store, &format!("k{i}"), None, t0);
    }
    let ttl = Duration::from_secs(bucket_ttl_secs(CONFIG));

    let (decision, _) = consume(&store, "later", None, t0 + ttl + Duration::from_secs(1));

    assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
    assert_eq!(store._bucket_count(), 1);
}
