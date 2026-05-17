#![cfg(test)]

use super::config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
use super::store::{InMemoryRateLimitStore, RateLimitStore};

#[tokio::test]
async fn allows_up_to_burst_then_throttles() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(3, 0.001); // very slow refill
    for i in 0..3 {
        let decision = store.consume("k", config).await.unwrap();
        assert!(
            matches!(decision, RateLimitDecision::Allowed { .. }),
            "attempt {i} should be allowed: {decision:?}",
        );
    }
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Throttled { .. }));
}

#[tokio::test]
async fn refill_grants_more_tokens_after_wait() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(2, 1000.0); // refills instantly
    // exhaust
    store.consume("k", config).await.unwrap();
    store.consume("k", config).await.unwrap();
    // sleep a hair, then expect refill to allow another
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
}

#[tokio::test]
async fn per_key_isolation_does_not_leak_between_principals() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(1, 0.001);
    let a = store.consume("alice", config).await.unwrap();
    let b = store.consume("bob", config).await.unwrap();
    assert!(matches!(a, RateLimitDecision::Allowed { .. }));
    assert!(matches!(b, RateLimitDecision::Allowed { .. }));
    let a_throttled = store.consume("alice", config).await.unwrap();
    assert!(matches!(a_throttled, RateLimitDecision::Throttled { .. }));
}

#[test]
fn capacity_helper_passes_burst() {
    assert_eq!(_bucket_capacity_for(RateLimitConfig::new(7, 1.0)), 7);
}
