#![cfg(test)]
//! Integration tests for `handle_baseline` (issue #205), split by
//! scenario into sibling submodules to stay under the 200-LoC budget
//! (mirrors `cratestack-migrate`'s `diff/tests.rs` + `diff/tests/*.rs`
//! layout). Mapped directly to design doc
//! `docs/design/migrate-baseline.md` §8's test plan / originating
//! issue #135's acceptance criteria.
//!
//! Backend selection mirrors the standard shim every other PG-backed test
//! in the workspace uses (`crates/cratestack-pg/tests/support/pg.rs`):
//! `CRATESTACK_TEST_DATABASE_URL` (external PG, `just test-pg`'s flow),
//! else `CRATESTACK_USE_TESTCONTAINERS=1` (ephemeral PG container, shared
//! for the whole binary — started once, lazily, by the first test that
//! needs it), else skip. `CRATESTACK_REQUIRE_DB` turns a connection/
//! container failure into a panic instead of a skip.
//!
//! A 2026-08 CI-coverage audit found `isolated_test_db` previously only
//! ever checked `CRATESTACK_TEST_DATABASE_URL` — no testcontainers
//! fallback, no `CRATESTACK_REQUIRE_DB` hard-fail, unlike every other PG
//! test here — and CI never set that variable, so 4 of these 5 tests
//! silently skipped on every run.
//!
//! Each test connects with its own dedicated Postgres *schema*
//! (`search_path` pinned via the connection URL's `options` query
//! parameter — supported by `sqlx-postgres`'s URL parser), so
//! `baseline`'s whole-`current_schema()` introspection can't see (or
//! be confused by) tables any other concurrently-running test creates
//! in the default `public` schema.

mod apply_pending;
mod clean;
mod drift;
mod refuses;
mod regression;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use tempfile::TempDir;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PostgresContainer;

pub(super) fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
    let path = dir.path().join("schema.cstack");
    fs::write(&path, source).expect("write schema");
    path
}

pub(super) fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(fut)
}

/// Collapse a `Result` into `Option`, but panic instead of skipping when
/// `CRATESTACK_REQUIRE_DB` is set — same convention as
/// `crates/cratestack-pg/tests/support/pg.rs`'s `need`.
fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
        Err(_) => None,
    }
}

/// The testcontainers backend's container, started once (lazily, on the
/// first test that needs it) and shared for the rest of this binary's
/// tests — each still gets its own isolated schema via `isolated_test_db`.
/// `None` means either the backend isn't selected or starting it failed
/// (and `CRATESTACK_REQUIRE_DB` wasn't set, so the caller skips).
fn testcontainer() -> &'static Option<ContainerAsync<PostgresContainer>> {
    static CONTAINER: OnceLock<Option<ContainerAsync<PostgresContainer>>> = OnceLock::new();
    CONTAINER.get_or_init(|| {
        let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();
        block_on(async {
            need(
                PostgresContainer::default().start().await,
                require,
                "starting the Postgres testcontainer (is Docker available?)",
            )
        })
    })
}

/// Base connection URL (no schema pinned yet) for whichever backend is
/// selected, or `None` if neither is configured (or the testcontainers
/// backend failed to start and `CRATESTACK_REQUIRE_DB` isn't set).
fn base_url() -> Option<String> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    if let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        return Some(url);
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = testcontainer().as_ref()?;
        return block_on(async {
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
            Some(format!(
                "postgres://postgres:postgres@{host}:{port}/postgres"
            ))
        });
    }

    if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is set"
        );
    }

    None
}

/// `None` (test skips) unless a database backend is configured — see the
/// module docs. Otherwise the isolated URL: `search_path` pinned to a
/// schema dedicated to `test_name`, created fresh (dropped and recreated)
/// so repeated runs start clean.
pub(super) fn isolated_test_db(test_name: &str) -> Option<String> {
    let base_url = base_url()?;
    let schema_name = format!("cli_baseline_test_{test_name}");
    let isolated_url = format!("{base_url}?options=-c%20search_path%3D{schema_name}");

    block_on(async {
        let pool = PoolOptions::<Postgres>::new()
            .max_connections(2)
            .connect(&base_url)
            .await
            .expect("connect to set up isolated schema");
        sqlx_core::raw_sql::raw_sql(&format!(
            "DROP SCHEMA IF EXISTS {schema_name} CASCADE; CREATE SCHEMA {schema_name};"
        ))
        .execute(&pool)
        .await
        .expect("create isolated test schema");
    });

    Some(isolated_url)
}

pub(super) async fn connect(url: &str) -> PgPool {
    PoolOptions::<Postgres>::new()
        .max_connections(2)
        .connect(url)
        .await
        .expect("connect to isolated schema")
}

pub(super) async fn exec(pool: &PgPool, sql: &str) {
    sqlx_core::raw_sql::raw_sql(sql)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("DDL failed: {sql}\n{error}"));
}

pub(super) fn migration_dirs(backend_dir: &Path) -> Vec<PathBuf> {
    if !backend_dir.exists() {
        return Vec::new();
    }
    fs::read_dir(backend_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect()
}

pub(super) const WIDGET_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Widget {
  id Int @id
  name String @unique
}
"#;

pub(super) const WIDGET_SCHEMA_WITH_DESCRIPTION: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Widget {
  id Int @id
  name String @unique
  description String?
}
"#;
