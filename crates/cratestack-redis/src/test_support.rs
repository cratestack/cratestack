//! `CRATESTACK_REQUIRE_REDIS` guard for this crate's own `#[cfg(test)]`
//! unit tests (e.g. `idempotency::tests_store`, `ratelimit::tests_store`).
//!
//! These tests live inside the library's `src/` tree, in the same
//! compilation unit as `cratestack-redis` itself, so they can't reach
//! `tests/support/redis.rs` — that module only exists inside the separate
//! integration-test binaries under `tests/`. This mirrors just the
//! URL-gated skip/require behavior from `connect_or_skip` (issue #418) so
//! a misconfigured `CRATESTACK_REQUIRE_REDIS=1` run without
//! `CRATESTACK_REDIS_TEST_URL` fails loud here too, instead of silently
//! skipping while every other Redis test in CI panics.

#![cfg(test)]

/// Returns the configured Redis test URL, or `None` to signal the caller
/// should skip — unless `CRATESTACK_REQUIRE_REDIS` is set, in which case a
/// missing URL panics instead of skipping silently.
pub(crate) fn redis_test_url_or_skip() -> Option<String> {
    match std::env::var("CRATESTACK_REDIS_TEST_URL") {
        Ok(url) => Some(url),
        Err(_) if std::env::var("CRATESTACK_REQUIRE_REDIS").is_ok() => panic!(
            "CRATESTACK_REQUIRE_REDIS is set but CRATESTACK_REDIS_TEST_URL is unset — \
             misconfigured CI job would otherwise skip this test silently"
        ),
        Err(_) => None,
    }
}
