//! Test-PG backend selection — copied from `cratestack-pg/tests/support/pg.rs`
//! (same shape, adjusted to go straight through `cratestack_sqlx::sqlx`
//! since this crate does not depend on the `cratestack` facade). See that
//! file for the full rationale; summarized here:
//!
//! 1. `CRATESTACK_TEST_DATABASE_URL` — connect to an external PG (the
//!    `just pg-up` / `just test-pg` flow).
//! 2. `CRATESTACK_USE_TESTCONTAINERS=1` — spawn an ephemeral PG container.
//! 3. Neither set — return `None`; the caller skips.
//!
//! `CRATESTACK_REQUIRE_DB` turns a connection failure — and, since
//! cratestack#747, "neither variable set at all" — into a panic instead of
//! a skip (the CI gate sets it, so a broken Docker doesn't silently green a
//! suite that ran none of this coverage). That decision lives in
//! [`super::require_db`], kept pure so it is unit-testable; read its docs
//! for the #747 history.

use cratestack_sqlx::sqlx::PgPool;
use cratestack_sqlx::sqlx::postgres::PgPoolOptions;
use std::sync::OnceLock;

use super::require_db::Backend;
use super::require_db::pick_backend;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, MutexGuard};

pub struct TestPg {
    pub pool: PgPool,
    _container: Option<ContainerAsync<Postgres>>,
}

pub async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();
    let has_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").is_ok();
    let use_testcontainers = std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    // `pick_backend` panics itself when `require` is set and neither
    // backend env var is — see its doc comment.
    match pick_backend(has_url, use_testcontainers, require) {
        Backend::Skip => None,
        Backend::Url => {
            let url =
                std::env::var("CRATESTACK_TEST_DATABASE_URL").expect("has_url implies var is set");
            let pool = need(
                PgPoolOptions::new().max_connections(2).connect(&url).await,
                require,
                "connecting to CRATESTACK_TEST_DATABASE_URL",
            )?;
            Some(TestPg {
                pool,
                _container: None,
            })
        }
        Backend::TestContainers => {
            let container = need(
                // Tag pinned explicitly: `testcontainers-modules` hardcodes
                // `postgres:11-alpine` as its default, EOL since 2023-11-09.
                // Kept in lockstep with `compose.yml`'s `postgres:18` so the
                // testcontainers backend (what CI runs) and the compose
                // backend (what `just test-pg` runs) exercise the same major.
                Postgres::default().with_tag("18-alpine").start().await,
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
                PgPoolOptions::new().max_connections(2).connect(&url).await,
                require,
                "connecting to the Postgres testcontainer",
            )?;
            Some(TestPg {
                pool,
                _container: Some(container),
            })
        }
    }
}

/// Per-binary serialization around DROP/CREATE TABLE racing on the
/// external-URL backend, where the whole test binary shares one database.
pub async fn serial_guard() -> MutexGuard<'static, ()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(())).lock().await
}
