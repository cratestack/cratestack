//! Test-Redis backend selection.
//!
//! Integration tests for idempotency and rate-limit stores gate on
//! `CRATESTACK_REDIS_TEST_URL` and skip silently when it is unset. This
//! module centralizes that logic and adds a loud-failure guard to match
//! the pattern Postgres tests use via `CRATESTACK_REQUIRE_DB` (see
//! `crates/cratestack-pg/tests/support/pg.rs`).
//!
//! Two Redis backends, chosen at runtime via environment variables:
//!
//! 1. **`CRATESTACK_REDIS_TEST_URL`** — connect to an external Redis at
//!    the URL given. This is the fast path for local dev (shared compose
//!    container, ability to `redis-cli` mid-test). Matching the Postgres
//!    pattern, this is the preferred backend.
//!
//! 2. **`CRATESTACK_USE_TESTCONTAINERS=1`** — spawn an ephemeral Redis
//!    container via `testcontainers`. The container is held in
//!    [`TestRedis`]; its `Drop` stops and removes the container, so each
//!    test binary gets its own isolated Redis and CI never leaks a
//!    container.
//!
//! 3. **Neither set** — return `None`. The caller skips the test (same
//!    behavior every Redis test already had).
//!
//! Priority: explicit URL wins (most useful for "I have this thing
//! already running"); testcontainers second; skip last.

use redis::Client;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

/// A live Redis connection for a test, plus (when the testcontainers
/// backend is in use) the container guard. The container is dropped — i.e.
/// stopped and removed — when this struct is dropped, so a test that
/// holds a `TestRedis` for the duration of its body gets automatic cleanup
/// for free.
pub struct TestRedis {
    #[allow(dead_code)]
    pub client: Client,
    /// Held only when we spawned the container ourselves. The Drop on
    /// `ContainerAsync` issues the `docker rm -f` equivalent, so we
    /// never leak containers.
    ///
    /// Field name is `_container` so we communicate "exists for its Drop,
    /// not for direct use" — clippy won't flag the unused-binding either.
    _container: Option<ContainerAsync<Redis>>,
}

/// Which Redis backend `connect_or_skip` should use, decided purely from
/// which environment variables are present — no I/O. Split out so the
/// require/skip/panic decision is unit-testable deterministically, without
/// mutating real process env vars (racy across parallel test threads) or
/// touching a real connection. See `require_guard.rs` for the tests.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Url,
    TestContainers,
    Skip,
}

/// Pure decision logic for [`connect_or_skip`]. Panics in the one case the
/// loud-failure guard exists to catch: `require` is set (CI opted into
/// `CRATESTACK_REQUIRE_REDIS`) but neither backend env var is — a
/// misconfigured job that would otherwise skip the whole Redis suite
/// silently and still report green.
pub(crate) fn pick_backend(has_url: bool, use_testcontainers: bool, require: bool) -> Backend {
    if has_url {
        Backend::Url
    } else if use_testcontainers {
        Backend::TestContainers
    } else if require {
        panic!(
            "CRATESTACK_REQUIRE_REDIS is set but neither CRATESTACK_REDIS_TEST_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is set — misconfigured CI job would otherwise \
             skip the whole Redis suite silently"
        );
    } else {
        Backend::Skip
    }
}

/// Connect to Redis, picking the backend by environment, or return `None`
/// to signal that the caller should skip.
///
/// See module docs for the priority order. By default, connect failures
/// map to `None` rather than panicking — so a misconfigured local machine
/// just skips quietly instead of failing the whole test run.
///
/// **CI override:** set `CRATESTACK_REQUIRE_REDIS` to turn those failures
/// (and a missing backend selection entirely — see [`pick_backend`]) into
/// hard panics. Without it, a CI runner whose Docker can't start the
/// testcontainer would skip every Redis-backed test and the suite would
/// pass green while exercising none of that coverage — so the CI gate sets
/// it.
pub async fn connect_or_skip() -> Option<TestRedis> {
    let require = std::env::var("CRATESTACK_REQUIRE_REDIS").is_ok();
    let has_url = std::env::var("CRATESTACK_REDIS_TEST_URL").is_ok();
    let use_testcontainers = std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok();

    // Collapse a Result into Option, but panic instead of skipping when a
    // Redis is required (CI). `ctx` names the failed step for the message.
    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_REDIS is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    // `pick_backend` panics itself when `require` is set and neither
    // backend env var is — see its doc comment. Every other outcome is
    // handled below by re-deriving the concrete value each branch needs.
    match pick_backend(has_url, use_testcontainers, require) {
        Backend::Skip => None,
        Backend::Url => {
            let url =
                std::env::var("CRATESTACK_REDIS_TEST_URL").expect("has_url implies var is set");
            let client = need(
                Client::open(url),
                require,
                "parsing CRATESTACK_REDIS_TEST_URL",
            )?;
            // Verify the connection succeeds before returning.
            let _ = need(
                client.get_connection(),
                require,
                "connecting to CRATESTACK_REDIS_TEST_URL",
            )?;
            Some(TestRedis {
                client,
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
            let url = format!("redis://{host}:{port}");
            let client = need(
                Client::open(url),
                require,
                "parsing Redis testcontainer URL",
            )?;
            // Verify the connection succeeds before returning.
            let _ = need(
                client.get_connection(),
                require,
                "connecting to the Redis testcontainer",
            )?;
            Some(TestRedis {
                client,
                _container: Some(container),
            })
        }
    }
}
