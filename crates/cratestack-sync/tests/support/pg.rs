//! Test-PG backend selection, as in `cratestack-outbox/tests/support/pg.rs`,
//! plus two additions: [`TestPg::fresh_database`] and [`serial_guard`].
//!
//! Every sync test gets its own database so the tables, sequences and
//! trigger functions never collide. That is *not* enough isolation on its
//! own: the pull gate reads `pg_snapshot_xmin`, and transaction ids are
//! cluster-wide, so a test that deliberately holds a transaction open
//! stalls every other test's pulls in every database on the server. Found
//! the hard way — run in parallel, the late-commit test made two unrelated
//! tests flake. Hence [`serial_guard`] as well.

use cratestack_sqlx::sqlx::postgres::PgPoolOptions;
use cratestack_sqlx::sqlx::{self, PgPool};
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use std::sync::OnceLock;

use tokio::sync::{Mutex, MutexGuard};

use super::require_db::{Backend, pick_backend};

/// Serializes the PG tests within one test binary (see the module doc).
pub async fn serial_guard() -> MutexGuard<'static, ()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(())).lock().await
}

pub struct TestPg {
    pub admin: PgPool,
    url: String,
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

    let (url, container) = match pick_backend(has_url, use_testcontainers, require) {
        Backend::Skip => return None,
        Backend::Url => (
            std::env::var("CRATESTACK_TEST_DATABASE_URL").expect("has_url implies var is set"),
            None,
        ),
        Backend::TestContainers => {
            let container = need(
                // Same pinned tag as the outbox and pg suites (`compose.yml`).
                Postgres::default().with_tag("18-alpine").start().await,
                require,
                "starting the Postgres testcontainer (is Docker available?)",
            )?;
            let host = need(container.get_host().await, require, "resolving host")?;
            let port = need(
                container.get_host_port_ipv4(5432).await,
                require,
                "resolving port",
            )?;
            (
                format!("postgres://postgres:postgres@{host}:{port}/postgres"),
                Some(container),
            )
        }
    };
    let admin = need(
        PgPoolOptions::new().max_connections(2).connect(&url).await,
        require,
        "connecting to the test database",
    )?;
    Some(TestPg {
        admin,
        url,
        _container: container,
    })
}

impl TestPg {
    /// Create an empty database and return a pool on it.
    pub async fn fresh_database(&self) -> PgPool {
        let name = format!("cratestack_sync_{}", uuid::Uuid::new_v4().simple());
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&self.admin)
            .await
            .expect("create test database");
        let (base, query) = match self.url.split_once('?') {
            Some((base, query)) => (base, format!("?{query}")),
            None => (self.url.as_str(), String::new()),
        };
        let server = base.rsplit_once('/').map_or(base, |(server, _)| server);
        PgPoolOptions::new()
            .max_connections(8)
            .connect(&format!("{server}/{name}{query}"))
            .await
            .expect("connect to fresh test database")
    }
}
