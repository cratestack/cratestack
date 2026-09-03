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

use support::ratelimit_budget::{buckets, budget, scan_keys, scopes, store_or_skip};

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

/// **cratestack#871 round-2, item 3**, first half: an actively-used member
/// keeps its slot, so its live bucket is always accounted for. (The other
/// half — an idle member ageing out — is
/// `an_idle_member_frees_its_slot_once_its_score_ages_out` below, and on
/// an injected clock in `cratestack-axum`'s `store/scopes.rs`.)
#[tokio::test]
async fn an_active_member_keeps_its_slot_and_the_cap_still_holds() {
    let Some((store, _prefix, _redis)) = store_or_skip("slide").await else {
        return;
    };
    let budget = budget(1);
    let config = RateLimitConfig::new(50, 0.001);

    // `a` is admitted and then used repeatedly.
    for _ in 0..5 {
        let outcome = store
            .consume_bounded(ConsumeRequest::new("auth:a", config, Some(&budget)))
            .await
            .expect("consume");
        assert_eq!(
            outcome.charged,
            Charged::Requested,
            "an active member must keep its slot",
        );
    }
    // ...so `b` cannot take the only slot while `a` is still using it.
    let outcome = store
        .consume_bounded(ConsumeRequest::new("auth:b", config, Some(&budget)))
        .await
        .expect("consume");
    assert_eq!(outcome.charged, Charged::Fallback);
}

/// The concurrency probe from the review: N concurrent requests for fresh
/// keys at a cap of 10 must admit EXACTLY 10, never more. This is what the
/// atomicity of the script buys — a read-then-write would let every one of
/// them observe "under budget".
#[tokio::test]
async fn concurrent_fresh_keys_admit_exactly_the_cap() {
    let Some((store, _prefix, _redis)) = store_or_skip("concurrent").await else {
        return;
    };
    const CAP: u32 = 10;
    const REQUESTS: usize = 200;
    let budget = std::sync::Arc::new(budget(CAP));
    let config = RateLimitConfig::new(50, 0.001);
    let store = std::sync::Arc::new(store);

    let mut tasks = Vec::new();
    for nonce in 0..REQUESTS {
        let (store, budget) = (store.clone(), budget.clone());
        tasks.push(tokio::spawn(async move {
            let key = format!("auth:c{nonce}");
            store
                .consume_bounded(ConsumeRequest::new(&key, config, Some(&budget)))
                .await
                .expect("consume")
                .charged
        }));
    }

    let mut requested = 0;
    let mut fallback = 0;
    for task in tasks {
        match task.await.expect("join") {
            Charged::Requested => requested += 1,
            Charged::Fallback => fallback += 1,
            other => panic!("unexpected charge {other:?}"),
        }
    }

    assert_eq!(requested, CAP as usize, "exactly the cap may be admitted");
    assert_eq!(fallback, REQUESTS - CAP as usize);
}

/// **cratestack#871 round-2, item 3**, second half: a slot whose
/// credential has gone quiet for `scope_ttl` is trimmed, so a peer whose
/// tokens rotate is never permanently capped.
///
/// `scope_ttl` is at least the 60s bucket-TTL floor, far too long to sleep
/// through in a test — so rather than waiting, the member's score is
/// rewritten to the epoch with `ZADD`, which is exactly the state the
/// clock would have produced. The assertion is then on the script's
/// `ZREMRANGEBYSCORE` doing its job.
#[tokio::test]
async fn an_idle_member_frees_its_slot_once_its_score_ages_out() {
    let Some((store, prefix, redis)) = store_or_skip("ageing").await else {
        return;
    };
    let budget = budget(1);
    let config = RateLimitConfig::new(50, 0.001);

    store
        .consume_bounded(ConsumeRequest::new("auth:old", config, Some(&budget)))
        .await
        .expect("consume");
    let blocked = store
        .consume_bounded(ConsumeRequest::new("auth:new", config, Some(&budget)))
        .await
        .expect("consume");
    assert_eq!(blocked.charged, Charged::Fallback, "the only slot is taken");

    let keys = scan_keys(&redis, &prefix).await;
    let scope_key = scopes(&keys)
        .first()
        .copied()
        .expect("a scope zset")
        .clone();
    let mut conn = redis
        .client
        .get_multiplexed_async_connection()
        .await
        .expect("connect");
    let members: Vec<String> = redis::cmd("ZRANGE")
        .arg(&scope_key)
        .arg(0)
        .arg(-1)
        .query_async(&mut conn)
        .await
        .expect("zrange");
    assert_eq!(members.len(), 1, "one admitted member: {members:?}");
    let _: i64 = redis::cmd("ZADD")
        .arg(&scope_key)
        .arg(0)
        .arg(&members[0])
        .query_async(&mut conn)
        .await
        .expect("zadd");

    let admitted = store
        .consume_bounded(ConsumeRequest::new("auth:new", config, Some(&budget)))
        .await
        .expect("consume");
    assert_eq!(
        admitted.charged,
        Charged::Requested,
        "an aged-out slot must be reclaimable, or a token-rotating peer is capped forever",
    );
}
