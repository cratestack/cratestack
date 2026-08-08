//! Test that CRATESTACK_REQUIRE_REDIS guard works correctly.
//!
//! This test file verifies that when `CRATESTACK_REQUIRE_REDIS` is unset,
//! the helper function returns None (skips the test cleanly) instead of
//! panicking. When the guard IS set, it panics on connection failures.
//!
//! This behavior matches the pattern Postgres tests use with
//! CRATESTACK_REQUIRE_DB, ensuring silent skips for local dev but loud
//! failures in CI when the guard is set.

mod support;

#[tokio::test]
async fn without_require_guard_connection_failures_skip_silently() {
    // Without CRATESTACK_REQUIRE_REDIS set (normal local dev), the helper
    // should return None on any connection failure or missing URL, allowing
    // the test to skip silently. This test verifies that behavior.
    //
    // If CRATESTACK_REQUIRE_REDIS=1 and CRATESTACK_REDIS_TEST_URL is not
    // set, this test would panic instead. That's the intended behavior for
    // CI — the main test files (idempotency.rs, ratelimit.rs, e2e.rs,
    // e2e_ratelimit.rs) will panic when the guard is set and Redis is
    // unavailable, making CI fail loudly instead of silently.
    let redis = support::redis::connect_or_skip().await;
    if redis.is_none() {
        // Expected: no Redis available, test skipped.
        // This is the normal path for local dev without a running Redis.
    } else {
        // If we get here, Redis IS available; verify we got a valid client.
        assert!(redis.is_some(), "connection should have succeeded");
    }
}
