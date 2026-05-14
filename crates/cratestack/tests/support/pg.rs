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
/// See module docs for the priority order. Connect failures map to
/// `None` rather than panicking — same as every existing
/// `connect_or_skip()` in the workspace — so a misconfigured local
/// machine just skips quietly instead of failing the whole test run.
pub async fn connect_or_skip() -> Option<TestPg> {
    if let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;
        return Some(TestPg {
            pool,
            _container: None,
        });
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = Postgres::default().start().await.ok()?;
        let host = container.get_host().await.ok()?;
        let port = container.get_host_port_ipv4(5432).await.ok()?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;
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
