//! Test-PG backend selection.
//!
//! Integration tests historically each carried their own copy of
//! `connect_or_skip()` + `serial_guard()`, all hitting a single shared
//! Postgres exposed via `compose.yml`. That worked but made cross-binary
//! parallelism a convention (every fixture must pick uniquely-named
//! tables) rather than a guarantee, and forced contributors to remember
//! `docker compose up` / `docker compose down` either side of a run.
//!
//! This module centralises both concerns and offers two PG backends,
//! chosen at runtime via environment variables:
//!
//! 1. **`CRATESTACK_TEST_DATABASE_URL`** — connect to an external PG at
//!    the URL given. This is the fast path for local dev (shared compose
//!    container, ability to `psql` mid-test). Existing `just test-pg`
//!    flow uses this.
//!
//! 2. **`CRATESTACK_USE_TESTCONTAINERS=1`** — spawn an ephemeral PG
//!    container via `testcontainers`. The container is held in
//!    [`TestPg`]; its `Drop` stops and removes the container, so each
//!    test binary gets its own isolated database and CI never leaks a
//!    container.
//!
//! 3. **Neither set** — return `None`. The caller skips the test (same
//!    behavior every `banking_*.rs` already had).
//!
//! Priority: explicit URL wins (most useful for "I have this thing
//! already running"); testcontainers second; skip last.

use std::sync::OnceLock;

use cratestack::sqlx::PgPool;
use cratestack::sqlx::postgres::PgPoolOptions;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;

/// A live PG connection for a test, plus (when the testcontainers backend
/// is in use) the container guard. The container is dropped — i.e.
/// stopped and removed — when this struct is dropped, so a test that
/// holds a `TestPg` for the duration of its body gets automatic cleanup
/// for free.
pub struct TestPg {
    pub pool: PgPool,
    /// Held only when we spawned the container ourselves. The Drop on
    /// `ContainerAsync` issues the `docker rm -f` equivalent, so we
    /// never leak containers.
    ///
    /// Field name is `_container` so we communicate "exists for its Drop,
    /// not for direct use" — clippy won't flag the unused-binding either.
    _container: Option<ContainerAsync<Postgres>>,
}

/// Connect to PG, picking the backend by environment, or return `None`
/// to signal that the caller should skip.
///
/// See module docs for the priority order. By default, connect failures
/// map to `None` rather than panicking — so a misconfigured local machine
/// just skips quietly instead of failing the whole test run.
///
/// **CI override:** set `CRATESTACK_REQUIRE_DB` to turn those failures
/// into hard panics. Without it, a CI runner whose Docker can't start the
/// testcontainer would skip every PG-backed test and the suite would pass
/// green while exercising none of that coverage — so the CI gate sets it.
pub async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    // Collapse a Result into Option, but panic instead of skipping when a
    // DB is required (CI). `ctx` names the failed step for the message.
    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    if let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        let pool = need(
            PgPoolOptions::new().max_connections(2).connect(&url).await,
            require,
            "connecting to CRATESTACK_TEST_DATABASE_URL",
        )?;
        return Some(TestPg {
            pool,
            _container: None,
        });
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = need(
            Postgres::default().start().await,
            require,
            "starting the Postgres testcontainer (is Docker available?)",
        )?;
        let host = need(container.get_host().await, require, "resolving testcontainer host")?;
        let port = need(
            container.get_host_port_ipv4(5432).await,
            require,
            "resolving testcontainer port",
        )?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let pool = need(
            PgPoolOptions::new().max_connections(2).connect(&url).await,
            require,
            "connecting to the Postgres testcontainer",
        )?;
        return Some(TestPg {
            pool,
            _container: Some(container),
        });
    }

    None
}

/// Per-binary serialisation around DROP/CREATE TABLE racing.
///
/// On the **external-URL backend** the whole test binary shares one
/// database, so two tests in the same file fighting over the same
/// `cratestack_audit` row need a mutex around their critical sections.
/// On the **testcontainers backend** each test binary already has its
/// own PG (one container per binary), so this is logically a no-op —
/// but keeping the same shape means individual tests don't have to know
/// which backend they're running under. Cost is negligible.
///
/// Held for the whole test body via `let _guard = serial_guard().await;`.
pub async fn serial_guard() -> MutexGuard<'static, ()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(())).lock().await
}
