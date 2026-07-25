#![cfg(test)]

use redis::AsyncCommands;

use super::store::RedisRateLimitStore;

#[tokio::test]
async fn connection_is_lazy_and_cached_after_first_success() {
    let store =
        RedisRateLimitStore::open("redis://127.0.0.1:1", "cratestack:test:rl-lazy").expect("open");
    assert!(
        store.conn.get().is_none(),
        "connection must not be established until first use"
    );
}

#[tokio::test]
async fn failed_connection_attempt_is_not_cached_so_next_call_can_retry() {
    // Port 1 is a reserved, unreachable port, so every connection attempt
    // against it fails deterministically without needing a live Redis.
    let store =
        RedisRateLimitStore::open("redis://127.0.0.1:1", "cratestack:test:rl-retry").expect("open");

    let first = store.connection().await;
    assert!(
        first.is_err(),
        "connecting to an unreachable Redis must fail"
    );
    assert!(
        store.conn.get().is_none(),
        "a failed connection attempt must not be cached — otherwise the store would fail \
         every subsequent call forever instead of retrying once Redis is reachable again",
    );

    let second = store.connection().await;
    assert!(
        second.is_err(),
        "the retry should also fail against the same unreachable host"
    );
}

/// Regression test for the underlying bug: `connection()` used to call
/// `get_multiplexed_async_connection()` fresh on every invocation instead
/// of reusing a cached connection. We can't reliably observe that via
/// Redis's `connected_clients` counter (short-lived connections close
/// again before a snapshot can catch them), so instead we tag the
/// connection returned by the first call with `CLIENT SETNAME` and check
/// that a second call round-trips through the *same* tagged connection
/// rather than a fresh, unnamed one.
#[tokio::test]
async fn connection_is_reused_not_reopened_per_call() {
    let Some(url) = std::env::var("CRATESTACK_REDIS_TEST_URL").ok() else {
        return;
    };
    let store = RedisRateLimitStore::open(url, "cratestack:test:rl-reuse-identity").expect("open");

    let marker = format!("cratestack-test-{}", uuid::Uuid::new_v4().simple());
    let mut first = store.connection().await.expect("first connection");
    let _: () = first.client_setname(&marker).await.expect("CLIENT SETNAME");

    let mut second = store.connection().await.expect("second connection");
    let name: Option<String> = second.client_getname().await.expect("CLIENT GETNAME");

    assert_eq!(
        name.as_deref(),
        Some(marker.as_str()),
        "expected connection() to return the same underlying Redis connection on repeated \
         calls, but a second call landed on a connection with no matching name — looks like \
         a new connection is being opened per call again",
    );
}
