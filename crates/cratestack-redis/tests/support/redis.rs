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

/// Connect to Redis, picking the backend by environment, or return `None`
/// to signal that the caller should skip.
///
/// See module docs for the priority order. By default, connect failures
/// map to `None` rather than panicking — so a misconfigured local machine
/// just skips quietly instead of failing the whole test run.
///
/// **CI override:** set `CRATESTACK_REQUIRE_REDIS` to turn those failures
/// into hard panics. Without it, a CI runner whose Docker can't start the
/// testcontainer would skip every Redis-backed test and the suite would
/// pass green while exercising none of that coverage — so the CI gate sets
/// it.
pub async fn connect_or_skip() -> Option<TestRedis> {
    let require = std::env::var("CRATESTACK_REQUIRE_REDIS").is_ok();

    // Collapse a Result into Option, but panic instead of skipping when a
    // Redis is required (CI). `ctx` names the failed step for the message.
    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_REDIS is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    if let Ok(url) = std::env::var("CRATESTACK_REDIS_TEST_URL") {
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
        return Some(TestRedis {
            client,
            _container: None,
        });
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = need(
            Redis::default().start().await,
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
        return Some(TestRedis {
            client,
            _container: Some(container),
        });
    }

    None
}
