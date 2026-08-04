#![cfg(test)]
//! Integration tests for `handle_baseline` (issue #205), split by
//! scenario into sibling submodules to stay under the 200-LoC budget
//! (mirrors `cratestack-migrate`'s `diff/tests.rs` + `diff/tests/*.rs`
//! layout). Mapped directly to design doc
//! `docs/design/migrate-baseline.md` §8's test plan / originating
//! issue #135's acceptance criteria.
//!
//! Every test that touches a live database skips silently unless
//! `CRATESTACK_TEST_DATABASE_URL` is set (`just test-pg` sets it —
//! same convention as every other PG-backed test in the workspace).
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

use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use tempfile::TempDir;

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

/// `None` (test skips) unless `CRATESTACK_TEST_DATABASE_URL` is set.
/// Otherwise the isolated URL: `search_path` pinned to a schema
/// dedicated to `test_name`, created fresh (dropped and recreated) so
/// repeated runs start clean.
pub(super) fn isolated_test_db(test_name: &str) -> Option<String> {
    let base_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
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
