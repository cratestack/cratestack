//! `CRATESTACK_REQUIRE_REDIS` guard for this crate's own `#[cfg(test)]`
//! unit tests (e.g. `idempotency::tests_store`, `ratelimit::tests_store`).
//!
//! These tests live inside the library's `src/` tree, in the same
//! compilation unit as `cratestack-redis` itself, so they can't reach
//! `tests/support/redis.rs` — that module only exists inside the separate
//! integration-test binaries under `tests/`. This mirrors the *full*
//! three-state backend selection from `connect_or_skip`/`pick_backend`
//! (issue #418): explicit URL, then `CRATESTACK_USE_TESTCONTAINERS`, then
//! skip — or panic loudly when `CRATESTACK_REQUIRE_REDIS` is set and
//! neither backend is configured.
//!
//! An earlier version of this module only implemented the URL-gated half
//! of that decision, so the blocking `tests-redis` CI job — which sets
//! `CRATESTACK_REQUIRE_REDIS=1` and `CRATESTACK_USE_TESTCONTAINERS=1` but
//! never an explicit URL — panicked on these two tests every single run.

#![cfg(test)]

use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

/// Which Redis backend [`redis_test_url_or_skip`] should use, decided
/// purely from which environment variables are present — no I/O. Split
/// out so the require/skip/panic decision is unit-testable
/// deterministically, without mutating real process env vars (racy across
/// parallel test threads). Mirrors `tests/support/redis.rs::pick_backend`;
/// duplicated rather than shared because unit tests under `src/` and
/// integration tests under `tests/` compile as separate crates.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Url,
    TestContainers,
    Skip,
}

pub(crate) fn pick_backend(has_url: bool, use_testcontainers: bool, require: bool) -> Backend {
    if has_url {
        Backend::Url
    } else if use_testcontainers {
        Backend::TestContainers
    } else if require {
        panic!(
            "CRATESTACK_REQUIRE_REDIS is set but neither CRATESTACK_REDIS_TEST_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is set — misconfigured CI job would otherwise \
             skip this test silently"
        );
    } else {
        Backend::Skip
    }
}

/// A live Redis URL for a test, plus (when the testcontainers backend is
/// in use) the container guard. The container is stopped and removed when
/// this is dropped, so a test that holds the guard for its duration gets
/// automatic cleanup for free.
pub(crate) struct TestRedisUrl {
    pub(crate) url: String,
    #[allow(dead_code)]
    _container: Option<ContainerAsync<Redis>>,
}

/// Returns a live Redis test URL, or `None` to signal the caller should
/// skip. See module docs for the priority order (explicit URL, then
/// testcontainers) and the `CRATESTACK_REQUIRE_REDIS` loud-failure
/// behavior.
pub(crate) async fn redis_test_url_or_skip() -> Option<TestRedisUrl> {
    let require = std::env::var("CRATESTACK_REQUIRE_REDIS").is_ok();
    let has_url = std::env::var("CRATESTACK_REDIS_TEST_URL").is_ok();
    let use_testcontainers = std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_REDIS is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    match pick_backend(has_url, use_testcontainers, require) {
        Backend::Skip => None,
        Backend::Url => {
            let url =
                std::env::var("CRATESTACK_REDIS_TEST_URL").expect("has_url implies var is set");
            Some(TestRedisUrl {
                url,
                _container: None,
            })
        }
        Backend::TestContainers => {
            let container = need(
                // Tag pinned explicitly: `testcontainers-modules` hardcodes
                // `redis:5.0` as its default, EOL since 2022. 7.4 rather than
                // 8.x to keep the behavioural delta from the old default small.
                Redis::default().with_tag("7.4").start().await,
                require,
                "starting the Redis testcontainer (is Docker available?)",
            )?;
            let host = need(
                container.get_host().await,
                require,
                "resolving testcontainer host",
            )?;
            let port = need(
                container.get_host_port_ipv4(6379).await,
                require,
                "resolving testcontainer port",
            )?;
            Some(TestRedisUrl {
                url: format!("redis://{host}:{port}"),
                _container: Some(container),
            })
        }
    }
}

#[cfg(test)]
mod pick_backend_tests {
    use super::Backend;
    use super::pick_backend;

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

    /// Regression test for the exact bug the `tests-redis` CI job hit: a
    /// job sets `CRATESTACK_REQUIRE_REDIS` and `CRATESTACK_USE_TESTCONTAINERS`
    /// but no explicit URL — this must resolve to `TestContainers`, not
    /// panic or skip.
    #[test]
    fn testcontainers_and_require_together_does_not_panic() {
        assert_eq!(pick_backend(false, true, true), Backend::TestContainers);
    }

    /// Regression test for the original guard-defeating bug: neither
    /// backend env var set but `CRATESTACK_REQUIRE_REDIS` is — this must
    /// panic instead of silently skipping.
    #[test]
    #[should_panic(expected = "CRATESTACK_REQUIRE_REDIS is set but neither")]
    fn neither_set_but_required_panics_instead_of_skipping() {
        let _ = pick_backend(false, false, true);
    }
}
