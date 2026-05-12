//! Integration tests for [`RedisRateLimitStore`].
//!
//! Mirrors the in-memory `InMemoryRateLimitStore` test scenarios in
//! `cratestack-axum::ratelimit::tests` so that the Redis backend
//! observes the same trait contract. Tests are skipped unless
//! `CRATESTACK_REDIS_TEST_URL` is set — matching the pattern used by
//! the idempotency integration tests in this crate.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitDecision, RateLimitStore};
use cratestack_redis::RedisRateLimitStore;
use uuid::Uuid;

fn store_or_skip(suffix: &str) -> Option<RedisRateLimitStore> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    // Per-test prefix so parallel test binaries (and the idempotency
    // tests in this same crate) can't trample each other.
    let prefix = format!("cratestack:test:rl:{suffix}:{}", Uuid::new_v4().simple());
    RedisRateLimitStore::open(url, prefix).ok()
}

fn raw_client_or_skip() -> Option<redis::Client> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    redis::Client::open(url).ok()
}

#[tokio::test]
async fn allows_up_to_burst_then_throttles() {
    let Some(store) = store_or_skip("burst") else {
        return;
    };
    // Very slow refill so the burst is effectively the only thing
    // available during the test window.
    let config = RateLimitConfig::new(3, 0.001);

    for i in 0..3 {
        let decision = store.consume("k", config).await.expect("consume");
        assert!(
            matches!(decision, RateLimitDecision::Allowed { .. }),
            "attempt {i} should be allowed, got {decision:?}",
        );
    }
    let decision = store.consume("k", config).await.expect("consume");
    assert!(
        matches!(decision, RateLimitDecision::Throttled { .. }),
        "post-burst attempt should be throttled, got {decision:?}",
    );
}

#[tokio::test]
async fn allowed_remaining_decreases_with_each_call() {
    let Some(store) = store_or_skip("remaining") else {
        return;
    };
    let config = RateLimitConfig::new(3, 0.001);

    let mut seen = Vec::new();
    for _ in 0..3 {
        match store.consume("k", config).await.expect("consume") {
            RateLimitDecision::Allowed { remaining } => seen.push(remaining),
            other => panic!("expected Allowed, got {other:?}"),
        }
    }
    // The reported `remaining` is the floor of `tokens` after the
    // decrement. With burst=3 and a near-zero refill we should see a
    // strictly non-increasing sequence ending at 0.
    assert_eq!(*seen.last().expect("at least one sample"), 0);
    for window in seen.windows(2) {
        assert!(
            window[0] >= window[1],
            "remaining must be non-increasing: {seen:?}",
        );
    }
}

#[tokio::test]
async fn refill_grants_more_tokens_after_wait() {
    let Some(store) = store_or_skip("refill") else {
        return;
    };
    let config = RateLimitConfig::new(2, 1000.0); // refills very fast

    store.consume("k", config).await.expect("first");
    store.consume("k", config).await.expect("second");

    tokio::time::sleep(Duration::from_millis(20)).await;
    let decision = store.consume("k", config).await.expect("after wait");
    assert!(
        matches!(decision, RateLimitDecision::Allowed { .. }),
        "refill should grant a token after the wait, got {decision:?}",
    );
}

#[tokio::test]
async fn per_key_isolation_does_not_leak_between_principals() {
    let Some(store) = store_or_skip("isolation") else {
        return;
    };
    let config = RateLimitConfig::new(1, 0.001);

    let alice = store.consume("alice", config).await.expect("alice 1");
    let bob = store.consume("bob", config).await.expect("bob 1");
    assert!(matches!(alice, RateLimitDecision::Allowed { .. }));
    assert!(matches!(bob, RateLimitDecision::Allowed { .. }));

    // Exhausting Alice's bucket must leave Bob's bucket untouched.
    let alice_throttled = store.consume("alice", config).await.expect("alice 2");
    assert!(matches!(alice_throttled, RateLimitDecision::Throttled { .. }));
    let bob_throttled = store.consume("bob", config).await.expect("bob 2");
    assert!(matches!(bob_throttled, RateLimitDecision::Throttled { .. }));
}

#[tokio::test]
async fn throttled_retry_after_is_bounded_for_typical_configs() {
    let Some(store) = store_or_skip("retry-after") else {
        return;
    };
    // Refill 1 token per second; after burst is consumed, the retry-
    // after must be a small positive integer rather than 0 or absurdly
    // large.
    let config = RateLimitConfig::new(1, 1.0);
    store.consume("k", config).await.expect("first");
    let decision = store.consume("k", config).await.expect("second");
    match decision {
        RateLimitDecision::Throttled { retry_after_secs } => {
            assert!(
                (1..=5).contains(&retry_after_secs),
                "retry_after_secs out of expected range: {retry_after_secs}",
            );
        }
        other => panic!("expected Throttled, got {other:?}"),
    }
}

#[tokio::test]
async fn zero_refill_keeps_throttling_indefinitely() {
    // A bucket with `refill_per_second = 0` is the degenerate
    // "consume-once-then-stop" mode. The script must not divide by
    // zero, and consecutive throttled calls must keep reporting a
    // positive retry_after.
    let Some(store) = store_or_skip("zero-refill") else {
        return;
    };
    let config = RateLimitConfig::new(1, 0.0);
    store.consume("k", config).await.expect("first allowed");
    for i in 0..3 {
        let decision = store.consume("k", config).await.expect("after burst");
        match decision {
            RateLimitDecision::Throttled { retry_after_secs } => {
                assert!(
                    retry_after_secs >= 1,
                    "iteration {i}: retry_after must be at least 1s, got {retry_after_secs}",
                );
            }
            other => panic!("iteration {i}: expected Throttled, got {other:?}"),
        }
    }
}

// -----------------------------------------------------------------------------
// Concurrency: parallel consume calls must respect the burst limit. We can't
// require the exact split (Redis Lua is atomic per script, but the test
// harness can interleave wall-clock time between launches), but the total
// number of "allowed" responses must be bounded by `burst` plus whatever the
// refill produced during the window — which we keep effectively zero.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_consumes_respect_burst_limit() {
    let Some(store) = store_or_skip("concurrent") else {
        return;
    };
    let store = Arc::new(store);
    let config = RateLimitConfig::new(5, 0.001); // negligible refill
    let allowed = Arc::new(AtomicUsize::new(0));
    let throttled = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();
    for _ in 0..20 {
        let store = Arc::clone(&store);
        let allowed = Arc::clone(&allowed);
        let throttled = Arc::clone(&throttled);
        tasks.push(tokio::spawn(async move {
            match store.consume("k", config).await.expect("consume") {
                RateLimitDecision::Allowed { .. } => {
                    allowed.fetch_add(1, Ordering::SeqCst);
                }
                RateLimitDecision::Throttled { .. } => {
                    throttled.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }
    for t in tasks {
        t.await.expect("task");
    }
    let allowed_n = allowed.load(Ordering::SeqCst);
    let throttled_n = throttled.load(Ordering::SeqCst);
    assert_eq!(
        allowed_n + throttled_n,
        20,
        "every task must produce exactly one decision",
    );
    // Allow a tiny refill margin (1 extra) so the test doesn't flake
    // on slow CI machines where the 20 calls span more than a few ms.
    assert!(
        allowed_n <= 6,
        "burst of 5 must not be exceeded by more than the refill margin; got {allowed_n} allowed",
    );
    assert!(
        allowed_n >= 5,
        "at least the burst of 5 must be allowed; got {allowed_n}",
    );
}

// -----------------------------------------------------------------------------
// Eviction: the bucket key must carry an EXPIRE so idle entries don't pile up.
// -----------------------------------------------------------------------------

async fn pttl_for(client: &redis::Client, key: &str) -> i64 {
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("conn");
    redis::cmd("PTTL")
        .arg(key)
        .query_async(&mut conn)
        .await
        .expect("PTTL")
}

#[tokio::test]
async fn consume_sets_expire_on_the_bucket_key() {
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let prefix = format!("cratestack:test:rl-ttl:{}", Uuid::new_v4().simple());
    let store = RedisRateLimitStore::from_client(client.clone(), prefix);
    let config = RateLimitConfig::new(10, 1.0); // refill window ≈ 10s + 60s margin

    store.consume("k", config).await.expect("consume");
    let bucket_key = store.bucket_key("k");
    let pttl = pttl_for(&client, &bucket_key).await;
    // PTTL returns -1 for no-expiry, -2 for missing. Both would mean
    // the bucket leaks memory.
    assert!(
        pttl > 0 && pttl <= 86_400_000,
        "PTTL must be in (0, 24h], got {pttl}",
    );
}

#[tokio::test]
async fn zero_refill_still_sets_an_expire() {
    // The `refill_per_second = 0` branch can't compute a refill window,
    // so the script falls back to a 24h TTL. Without that fallback the
    // bucket would never expire and we'd leak memory.
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let prefix = format!("cratestack:test:rl-zero-ttl:{}", Uuid::new_v4().simple());
    let store = RedisRateLimitStore::from_client(client.clone(), prefix);
    let config = RateLimitConfig::new(1, 0.0);

    store.consume("k", config).await.expect("consume");
    let bucket_key = store.bucket_key("k");
    let pttl = pttl_for(&client, &bucket_key).await;
    assert!(
        pttl > 0 && pttl <= 86_400_000,
        "zero-refill bucket must still have a TTL, got {pttl}",
    );
}

// -----------------------------------------------------------------------------
// Configured prefix is faithfully reflected in the Redis key.
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// Randomized properties. Seeded by `CRATESTACK_TEST_SEED` (default fixed),
// printed in every assertion message so a failure can be reproduced exactly.
// -----------------------------------------------------------------------------

fn test_seed() -> u64 {
    std::env::var("CRATESTACK_TEST_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed })
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi);
        lo + (self.next_u32() % (hi - lo + 1))
    }
    fn next_string(&mut self, max_len: usize) -> String {
        let len = (self.next_u32() as usize) % (max_len + 1);
        const ALPHABET: &[u8] = b"abcdefghij0123456789:\0 -_";
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let idx = (self.next_u32() as usize) % ALPHABET.len();
            s.push(ALPHABET[idx] as char);
        }
        s
    }
}

#[tokio::test]
async fn randomized_consume_grants_at_most_burst_tokens_in_a_short_window() {
    // For each random `(burst, key)` chosen by the PRNG, fire N=burst+5
    // consume calls back-to-back with a near-zero refill rate. The
    // store must allow exactly `burst` tokens (the refill margin is
    // negligible over the test window) and throttle the rest.
    let Some(store) = store_or_skip("rand-burst") else {
        return;
    };
    let store = Arc::new(store);
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..8 {
        let burst = rng.next_range_u32(1, 8);
        let key = format!("rand-key-{}", rng.next_u64());
        // Pick a refill so slow that elapsed * refill ≪ 1 over the
        // wall-clock window of the loop. 0.0001 tok/s over ~50ms = 5e-6.
        let config = RateLimitConfig::new(burst, 0.0001);

        let mut allowed = 0u32;
        let mut throttled = 0u32;
        for attempt in 0..(burst + 5) {
            match store.consume(&key, config).await.unwrap_or_else(|err| {
                panic!(
                    "seed={seed:#x} iter={iteration} burst={burst} attempt={attempt}: {err:?}",
                )
            }) {
                RateLimitDecision::Allowed { .. } => allowed += 1,
                RateLimitDecision::Throttled { .. } => throttled += 1,
            }
        }
        assert_eq!(
            allowed, burst,
            "seed={seed:#x} iter={iteration} burst={burst} key={key}: should allow exactly burst",
        );
        assert_eq!(
            throttled, 5,
            "seed={seed:#x} iter={iteration} burst={burst} key={key}: should throttle the overflow",
        );
    }
}

#[tokio::test]
async fn randomized_keys_have_independent_buckets() {
    // Pick a handful of random keys, exhaust each one, and verify that
    // exhausting one never throttles another. This is the multi-tenant
    // isolation property under a random workload.
    let Some(store) = store_or_skip("rand-iso") else {
        return;
    };
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    let config = RateLimitConfig::new(1, 0.0001);
    let keys: Vec<String> = (0..6).map(|_| rng.next_string(24)).collect();

    // First pass: each key gets its single allowed token.
    for (i, key) in keys.iter().enumerate() {
        let decision = store.consume(key, config).await.unwrap_or_else(|err| {
            panic!("seed={seed:#x} key#{i}={key:?}: {err:?}")
        });
        assert!(
            matches!(decision, RateLimitDecision::Allowed { .. }),
            "seed={seed:#x} key#{i}={key:?}: first consume must succeed, got {decision:?}",
        );
    }
    // Second pass: every key is now exhausted independently.
    for (i, key) in keys.iter().enumerate() {
        let decision = store.consume(key, config).await.unwrap_or_else(|err| {
            panic!("seed={seed:#x} key#{i}={key:?}: {err:?}")
        });
        assert!(
            matches!(decision, RateLimitDecision::Throttled { .. }),
            "seed={seed:#x} key#{i}={key:?}: second consume must throttle, got {decision:?}",
        );
    }
}

#[tokio::test]
async fn randomized_concurrent_consume_never_exceeds_burst() {
    // Fan out N concurrent tasks against the same key with a random
    // burst. The atomic Lua script must ensure `allowed ≤ burst + small
    // refill margin` no matter how the tasks interleave.
    let Some(store) = store_or_skip("rand-concurrent") else {
        return;
    };
    let store = Arc::new(store);
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..4 {
        let burst = rng.next_range_u32(2, 10);
        let tasks_n = rng.next_range_u32(burst + 4, burst + 12);
        let key = format!("rand-conc-{}", rng.next_u64());
        let config = RateLimitConfig::new(burst, 0.0001);

        let allowed = Arc::new(AtomicUsize::new(0));
        let throttled = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..tasks_n {
            let store = Arc::clone(&store);
            let allowed = Arc::clone(&allowed);
            let throttled = Arc::clone(&throttled);
            let key = key.clone();
            tasks.push(tokio::spawn(async move {
                match store.consume(&key, config).await.expect("consume") {
                    RateLimitDecision::Allowed { .. } => {
                        allowed.fetch_add(1, Ordering::SeqCst);
                    }
                    RateLimitDecision::Throttled { .. } => {
                        throttled.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }
        for t in tasks {
            t.await.expect("task");
        }
        let allowed_n = allowed.load(Ordering::SeqCst) as u32;
        let throttled_n = throttled.load(Ordering::SeqCst) as u32;
        assert_eq!(
            allowed_n + throttled_n,
            tasks_n,
            "seed={seed:#x} iter={iteration} burst={burst} tasks={tasks_n}: every task must report once",
        );
        // Allow a refill margin of 1 to keep CI flake-resistant — the
        // Lua script is atomic, but wall-clock time between scheduling
        // the first and last task can drip in a fraction of a token.
        assert!(
            allowed_n >= burst && allowed_n <= burst + 1,
            "seed={seed:#x} iter={iteration} burst={burst} tasks={tasks_n}: allowed={allowed_n} must be in [burst, burst+1]",
        );
    }
}

#[tokio::test]
async fn custom_prefix_is_used_for_the_redis_key() {
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let prefix = format!("custom:rl:{suffix}");
    let store = RedisRateLimitStore::from_client(client.clone(), prefix.clone());
    let config = RateLimitConfig::new(2, 1.0);
    store.consume("p", config).await.expect("consume");

    let expected_key = store.bucket_key("p");
    assert!(
        expected_key.starts_with(&format!("{prefix}:rl:")),
        "key {expected_key} should start with the configured prefix",
    );
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("conn");
    let exists: bool = redis::cmd("EXISTS")
        .arg(&expected_key)
        .query_async(&mut conn)
        .await
        .expect("EXISTS");
    assert!(exists, "Redis must hold a hash at the prefixed key");
}
