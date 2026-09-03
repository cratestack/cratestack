//! cratestack#871 round-2, item 2: the store's caps must bound the scope
//! index as well as the bucket map, and must refuse *before* interning
//! anything. A sibling file for the workspace's 200-line ceiling.

#![cfg(test)]

use std::time::{Duration, Instant};

use cratestack_core::{BucketBudget, ConsumeRequest, RateLimitConfig};

use super::store::InMemoryRateLimitStore;

const CONFIG: RateLimitConfig = RateLimitConfig {
    burst: 5,
    refill_per_second: 0.001,
};

/// **cratestack#871 round-2, item 2.** `max_buckets` bounded the bucket
/// map and nothing else: `Scopes::admit` ran BEFORE `Buckets::consume`
/// could refuse, so every refused request still interned a scope entry and
/// a member key. Measured on the round-1 fix: `max_buckets=10` ->
/// `buckets=10 scopes=5000`, each scope able to hold 128 keys for up to a
/// day.
#[test]
fn a_refused_request_leaves_no_scope_entry_behind() {
    const MAX: usize = 10;
    let store = InMemoryRateLimitStore::default().with_max_buckets(MAX);
    let now = Instant::now();

    let mut refused = 0;
    for peer in 0..5000 {
        // A distinct scope AND a distinct bucket per request — the shape
        // that made the two maps diverge.
        let budget = BucketBudget::new(
            format!("peer:{peer}"),
            format!("ip:{peer}"),
            128,
            Duration::from_secs(60),
        );
        let key = format!("auth:{peer}");
        if store
            ._consume_at(ConsumeRequest::new(&key, CONFIG, Some(&budget)), now)
            .is_err()
        {
            refused += 1;
        }
    }

    assert!(refused > 0, "the cap must actually engage in this probe");
    assert!(
        store._bucket_count() <= MAX,
        "buckets: {} > {MAX}",
        store._bucket_count(),
    );
    assert!(
        store._scope_count() <= MAX,
        "the scope map is unbounded: {} scopes for {} buckets at a cap of {MAX}",
        store._scope_count(),
        store._bucket_count(),
    );
}
