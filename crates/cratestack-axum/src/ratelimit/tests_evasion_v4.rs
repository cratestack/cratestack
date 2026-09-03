//! cratestack#871 round-2 blocker: IPv4-mapped IPv6 must not collapse
//! every IPv4 client into one bucket. A sibling of `tests_evasion.rs` for
//! the workspace's 200-line ceiling; shares its request fixtures.

#![cfg(test)]

use std::sync::Arc;

use cratestack_core::RateLimitConfig;

use super::layer::RateLimitLayer;
use super::store::InMemoryRateLimitStore;
use super::tests_evasion::{drive, request};

/// **Round-2 blocker.** Measured on the round-1 fix: a dual-stack listener
/// delivers IPv4 clients as `::ffff:a.b.c.d`, whose top groups are zero, so
/// `/64` aggregation put **200 distinct IPv4 clients into 1 bucket with 5
/// allowed** — every IPv4 caller sharing one budget, and any one of them
/// able to deny the rest. Strictly worse than the evasion it came from.
#[tokio::test]
async fn ipv4_mapped_clients_do_not_collapse_into_one_bucket() {
    let store = Arc::new(InMemoryRateLimitStore::default());
    let layer = RateLimitLayer::new(store.clone(), RateLimitConfig::new(5, 0.001));

    let allowed = drive(
        &layer,
        (0..200).map(|n| {
            // Exactly what `accept()` yields on `[::]:0` for a v4 client.
            request(&format!("[::ffff:198.51.100.{}]:1", n % 200), None)
        }),
    )
    .await;

    assert_eq!(
        store._bucket_count(),
        200,
        "200 distinct IPv4 clients must get 200 buckets, not one shared one",
    );
    assert_eq!(
        allowed, 200,
        "every one of 200 distinct clients has its own burst; {allowed} allowed means they are \
         sharing a bucket and starving each other",
    );
}
