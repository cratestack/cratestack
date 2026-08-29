//! Test-PG backend selection — same shape as
//! `cratestack-pg/tests/support/pg.rs` and `cratestack-outbox/tests/support/pg.rs`,
//! adjusted to go straight through `sqlx-core`/`sqlx-postgres` since this
//! crate depends on those directly rather than through the `cratestack`
//! facade (see `Cargo.toml`'s comment on the `cratestack-sqlx` dependency
//! for why).
//!
//! `postgres_routed_writes.rs` (cratestack#507) introduced this
//! testcontainers/`CRATESTACK_REQUIRE_DB` machinery inline, noting at the
//! time that the crate had no shared test-support module yet to put it
//! in. A CI audit later found the other three `tests/postgres_*.rs` files
//! never adopted it — they still only understood
//! `CRATESTACK_TEST_DATABASE_URL`, so setting `CRATESTACK_USE_TESTCONTAINERS`/
//! `CRATESTACK_REQUIRE_DB` in CI would have left them silently skipping
//! forever. Extracting the pattern here, and moving all four files onto
//! it, is what actually closes that gap instead of fixing it for one file
//! out of four.
//!
//! Two PG backends, chosen at runtime via environment variables:
//!
//! 1. **`CRATESTACK_TEST_DATABASE_URL`** — connect to an external PG at
//!    the URL given. `just test-pg` sets this.
//! 2. **`CRATESTACK_USE_TESTCONTAINERS=1`** — spawn an ephemeral PG
//!    container via `testcontainers`. The container is held in
//!    [`TestPg`]; its `Drop` stops and removes it, so each test binary
//!    gets its own isolated database and CI never leaks a container.
//! 3. **Neither set** — return `None`, and the caller skips the test.
//!
//! Priority: explicit URL wins; testcontainers second; skip last.

use std::sync::OnceLock;

use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PostgresImage;
use tokio::sync::{Mutex, MutexGuard};

/// A live PG connection for a test, plus (when the testcontainers backend
/// is in use) the container guard — dropping it stops and removes the
/// container, so a test that holds this for its whole body gets automatic
/// cleanup for free.
pub struct TestPg {
    pub pool: PgPool,
    /// Held only when we spawned the container ourselves. `ContainerAsync`'s
    /// `Drop` issues the `docker rm -f` equivalent, so we never leak
    /// containers. Field name is `_container` so the reader (and clippy)
    /// sees "exists for its Drop, not for direct use".
    _container: Option<ContainerAsync<PostgresImage>>,
}

/// Connect to PG, picking the backend by environment, or return `None` to
/// signal that the caller should skip.
///
/// See module docs for the priority order. By default, connect failures
/// map to `None` rather than panicking, so a misconfigured local machine
/// just skips quietly instead of failing the whole test run.
///
/// **CI override:** set `CRATESTACK_REQUIRE_DB` to turn those failures —
/// including neither variable being set at all — into hard panics. Without
/// it, a CI runner with a broken Docker (or a job that simply forgot to
/// set either variable) would skip every Postgres-backed test here and the
/// suite would report green while exercising none of that coverage. That
/// is exactly the gap a coverage audit found in `cratestack-studio`'s CI
/// job: these four test files were skipping silently on every run, which
/// is how the duplicate-column bug PR #553 fixed shipped in the first
/// place — its decisive coverage never actually ran.
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
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
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
            // Tag pinned explicitly: `testcontainers-modules` hardcodes
            // `postgres:11-alpine` as its default, EOL since 2023-11-09.
            // Kept in lockstep with `compose.yml`'s `postgres:18` so the
            // testcontainers backend (what CI runs) and the compose backend
            // (what `just test-pg` runs) agree.
            PostgresImage::default().with_tag("18-alpine").start().await,
            require,
            "starting the Postgres testcontainer (is Docker available?)",
        )?;
        let host = need(
            container.get_host().await,
            require,
            "resolving testcontainer host",
        )?;
        let port = need(
            container.get_host_port_ipv4(5432).await,
            require,
            "resolving testcontainer port",
        )?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let pool = need(
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
            require,
            "connecting to the Postgres testcontainer",
        )?;
        return Some(TestPg {
            pool,
            _container: Some(container),
        });
    }

    // Load-bearing, and audited as correct by cratestack#747: the same
    // trailing guard was MISSING in `cratestack-pg` and `cratestack-outbox`,
    // so their whole PG-backed suites reported `ok` in 0.00s with
    // `CRATESTACK_REQUIRE_DB=1` set. Both now carry it, extracted into a
    // pure `pick_backend` with a `#[should_panic]` regression test
    // (`crates/cratestack-pg/tests/support/require_db.rs` is the reference
    // copy and lists every sibling). This crate's copy is left inline
    // because it is already correct and has no `tests/require_guard.rs`
    // binary to host the proof; if you touch it, mirror the pg one.
    if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is — this decisive test must run against a real \
             Postgres, not skip silently"
        );
    }
    None
}

/// Per-binary serialisation around DROP/CREATE TABLE racing.
///
/// On the **external-URL backend** the whole test binary shares one
/// database, so two tests in the same file fighting over the same table
/// need a mutex around their critical sections. On the **testcontainers
/// backend** each test binary already has its own PG (one container per
/// binary), so this is logically a no-op — but keeping the same shape
/// means individual tests don't have to know which backend is running.
/// Cost is negligible.
///
/// Held for the whole test body via `let _guard = serial_guard().await;`.
pub async fn serial_guard() -> MutexGuard<'static, ()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(())).lock().await
}
