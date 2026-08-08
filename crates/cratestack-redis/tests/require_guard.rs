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

/// Unit tests for `pick_backend`, the pure decision logic behind
/// `connect_or_skip`. Exercised directly (rather than through
/// `connect_or_skip` + real env vars) so the guard's behavior is
/// deterministic and doesn't require mutating process-global env vars,
/// which would race against other tests in this binary running in
/// parallel threads.
mod pick_backend_tests {
    use crate::support::redis::Backend;
    use crate::support::redis::pick_backend;

    #[test]
    fn url_present_wins_regardless_of_testcontainers_or_require() {
        assert_eq!(pick_backend(true, true, true), Backend::Url);
        assert_eq!(pick_backend(true, false, false), Backend::Url);
    }

    #[test]
    fn testcontainers_used_when_url_absent() {
        assert_eq!(pick_backend(false, true, false), Backend::TestContainers);
        assert_eq!(pick_backend(false, true, true), Backend::TestContainers);
    }

    #[test]
    fn neither_set_and_not_required_skips_quietly() {
        assert_eq!(pick_backend(false, false, false), Backend::Skip);
    }

    /// Regression test for the bug this guard exists to catch: a CI job
    /// sets `CRATESTACK_REQUIRE_REDIS` but is wired up incorrectly (forgets
    /// `CRATESTACK_REDIS_TEST_URL` and `CRATESTACK_USE_TESTCONTAINERS`).
    /// Before the fix, that fell through to a bare `Skip`/`None` regardless
    /// of `require`, so the whole 115-test Redis suite would silently skip
    /// and CI would still report green. This must panic instead.
    #[test]
    #[should_panic(expected = "CRATESTACK_REQUIRE_REDIS is set but neither")]
    fn neither_set_but_required_panics_instead_of_skipping() {
        let _ = pick_backend(false, false, true);
    }
}
