//! cratestack#416 must survive cratestack#871, against a real Redis.
//!
//! The budget's whole risk is that it re-introduces the shared-bucket
//! collision #416 removed. It does not, and this is where that is
//! measured: callers admitted under the cap keep their own buckets, and
//! one exhausting its burst leaves another's untouched.
//!
//! A separate binary from `ratelimit_budget.rs` only because the two
//! together exceed the workspace's 200-line file ceiling; they share
//! `support::ratelimit_budget`.

mod support;

use cratestack_core::{
    Charged, ConsumeRequest, RateLimitConfig, RateLimitDecision, RateLimitStore,
};

use support::ratelimit_budget::{buckets, budget, scan_keys, store_or_skip};

#[tokio::test]
async fn distinct_bearers_under_the_budget_do_not_share_a_bucket() {
    let Some((store, prefix, redis)) = store_or_skip("isolation").await else {
        return;
    };
    let budget = budget(8);
    let config = RateLimitConfig::new(2, 0.001);

    for user in 0..8 {
        let key = format!("auth:user{user}");
        let outcome = store
            .consume_bounded(ConsumeRequest::new(&key, config, Some(&budget)))
            .await
            .expect("consume");
        assert_eq!(
            outcome.charged,
            Charged::Requested,
            "user{user} is under the cap and must get its own bucket",
        );
    }

    // user0 drains its burst; user7 must be untouched by that.
    for _ in 0..2 {
        store
            .consume_bounded(ConsumeRequest::new("auth:user0", config, Some(&budget)))
            .await
            .expect("consume");
    }
    let user0 = store
        .consume_bounded(ConsumeRequest::new("auth:user0", config, Some(&budget)))
        .await
        .expect("consume");
    assert!(
        matches!(user0.decision, RateLimitDecision::Throttled { .. }),
        "user0 spent burst=2 over three calls and must be throttled, got {:?}",
        user0.decision,
    );

    let user7 = store
        .consume_bounded(ConsumeRequest::new("auth:user7", config, Some(&budget)))
        .await
        .expect("consume");
    assert_eq!(
        user7.decision,
        RateLimitDecision::Allowed { remaining: 0 },
        "user7 had one token left; user0's exhaustion must not have taken it",
    );

    let keys = scan_keys(&redis, &prefix).await;
    assert_eq!(
        buckets(&keys).len(),
        8,
        "eight callers under the cap, eight buckets: {keys:?}",
    );
}

/// Past the cap the collapse must be a *throttle*, charged to the peer's
/// own fallback bucket — not a refusal (which would hand an attacker a
/// deterministic outage) and not a free pass.
#[tokio::test]
async fn callers_past_the_cap_are_charged_to_the_fallback() {
    let Some((store, prefix, redis)) = store_or_skip("fallback").await else {
        return;
    };
    let budget = budget(2);
    let config = RateLimitConfig::new(2, 0.001);

    let mut charged = Vec::new();
    for user in 0..6 {
        let key = format!("auth:user{user}");
        charged.push(
            store
                .consume_bounded(ConsumeRequest::new(&key, config, Some(&budget)))
                .await
                .expect("consume")
                .charged,
        );
    }

    assert_eq!(&charged[..2], &[Charged::Requested, Charged::Requested]);
    assert!(
        charged[2..].iter().all(|c| *c == Charged::Fallback),
        "everything past the cap must take the fallback: {charged:?}",
    );

    let keys = scan_keys(&redis, &prefix).await;
    assert_eq!(
        buckets(&keys).len(),
        3,
        "two admitted buckets plus the shared fallback: {keys:?}",
    );
}
