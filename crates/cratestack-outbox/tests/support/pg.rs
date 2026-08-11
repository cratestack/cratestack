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
//! `CRATESTACK_REQUIRE_DB` turns a connection failure into a panic instead
//! of a skip (the CI gate sets it, so a broken Docker doesn't silently
//! green a suite that ran none of this coverage).

use cratestack_sqlx::sqlx::PgPool;
use cratestack_sqlx::sqlx::postgres::PgPoolOptions;
use std::sync::OnceLock;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, MutexGuard};

pub struct TestPg {
    pub pool: PgPool,
    _container: Option<ContainerAsync<Postgres>>,
}

pub async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

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
        return Some(TestPg {
            pool,
            _container: Some(container),
        });
    }

    None
}

/// Per-binary serialization around DROP/CREATE TABLE racing on the
/// external-URL backend, where the whole test binary shares one database.
pub async fn serial_guard() -> MutexGuard<'static, ()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(())).lock().await
}
