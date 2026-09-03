//! Real-Postgres proof for cratestack#867: the declared `query` returns
//! the right aggregates for an admitted principal, and `Forbidden` for one
//! the policy does not admit.
//!
//! **What this file does and does not prove about `sqlx`:** the crate's
//! `Cargo.toml` never lists `sqlx` as a dependency — every
//! `cratestack::sqlx::` path below resolves through this crate's one real
//! dependency, `cratestack` (`cratestack-pg`), the same re-export
//! `crates/cratestack-pg/tests/banking_*.rs` uses for raw fixture setup.
//! This file *does* use that path for table DDL and seeding, same as those
//! framework-internal tests do — that's expected and fine. The
//! acceptance-bar claim ("no `sqlx` line in `Cargo.toml`, no `sqlx::` path
//! and no SQL string in `src/`") is about
//! [`declarative_query_verification::monthly_loyalty_fees`], which this
//! file only calls, never reimplements.
//!
//! Skips (prints `ok` without exercising anything) if Docker isn't
//! available — set `CRATESTACK_REQUIRE_DB=1` to turn that into a hard
//! panic instead, same convention as `crates/cratestack-pg/tests/support/
//! pg.rs`. Read `finished in` rather than the summary line to tell a skip
//! from a pass: a real run takes seconds, a skipped one reports `0.00s`.

use cratestack::CratestackError;
use declarative_query_verification::{attempt_write, monthly_loyalty_fees, schema};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    let container = need(
        // Tag pinned explicitly: `testcontainers-modules` hardcodes
        // `postgres:11-alpine` as its default, EOL since 2023-11-09. Kept in
        // lockstep with `compose.yml`'s `postgres:18` so the testcontainers
        // backend (what CI runs) and the compose backend (what `just test-pg`
        // runs) exercise the same major.
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
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = need(
        cratestack::sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await,
        require,
        "connecting to the testcontainer",
    )?;
    // Leak the container so it stays alive for the test body — this test
    // binary runs one test per process invocation via `cargo test`'s
    // default harness, so the leak is bounded and cleaned up when the
    // process exits.
    std::mem::forget(container);
    Some(pool)
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query(
        "CREATE TABLE IF NOT EXISTS loyalty_fee_events (
            id BIGINT PRIMARY KEY,
            user_id TEXT NOT NULL,
            discount BIGINT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create table");
    cratestack::sqlx::query(
        "INSERT INTO loyalty_fee_events (id, user_id, discount, created_at) VALUES
            (1, 'user-7', 100, TIMESTAMPTZ '2026-01-10 00:00:00Z'),
            (2, 'user-7', 250, TIMESTAMPTZ '2026-03-05 00:00:00Z'),
            (3, 'user-7',  30, TIMESTAMPTZ '2026-03-20 00:00:00Z'),
            (4, 'user-9', 999, TIMESTAMPTZ '2026-03-21 00:00:00Z')",
    )
    .execute(pool)
    .await
    .expect("seed");
}

fn operator(subject: &str) -> cratestack::CratestackContext {
    cratestack::CratestackContext::authenticated([(
        "subjectId".to_owned(),
        cratestack::Value::String(subject.to_owned()),
    )])
}

fn cutoff() -> cratestack::chrono::DateTime<cratestack::chrono::Utc> {
    "2026-03-01T00:00:00Z"
        .parse()
        .expect("cutoff should parse")
}

#[tokio::test]
async fn the_declared_query_returns_both_aggregates() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    seed(&pool).await;

    let db = schema::Cratestack::builder(pool.clone()).build();
    let summary = monthly_loyalty_fees(&db, &operator("user-7"), "user-7".to_owned(), cutoff())
        .await
        .expect("an admitted principal should get rows");

    // 100 + 250 + 30 across all time; only 250 + 30 land on/after the
    // cutoff, so a `FILTER` clause that stopped applying would show up
    // here as `thisMonth == 380`.
    assert_eq!(summary.total, 380);
    assert_eq!(summary.thisMonth, 280);
}

#[tokio::test]
async fn the_policy_gates_the_call_before_any_sql_runs() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    seed(&pool).await;

    let db = schema::Cratestack::builder(pool.clone()).build();
    let outcome = monthly_loyalty_fees(
        &db,
        // Authenticated, but as somebody else: the schema's `@allow`
        // compares `auth().subjectId` against the query's own `userId`.
        &operator("user-9"),
        "user-7".to_owned(),
        cutoff(),
    )
    .await;

    assert!(
        matches!(outcome, Err(CratestackError::Forbidden(_))),
        "expected Forbidden, got {outcome:?}",
    );
}

#[tokio::test]
async fn a_query_cannot_write_even_when_the_policy_admits_the_caller() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    seed(&pool).await;

    let db = schema::Cratestack::builder(pool.clone()).build();
    // Note the principal: one the `@allow` ADMITS. Policy is not what
    // stops this — the read-only transaction is.
    let outcome = attempt_write(&db, &operator("user-7"), "user-7".to_owned()).await;

    assert!(
        outcome.is_err(),
        "a query body must not be able to write, got {outcome:?}",
    );

    // The decisive half. An error alone would also be produced by a
    // statement that inserted and *then* failed, so the row count is what
    // actually proves nothing landed.
    let count: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM loyalty_fee_events")
            .fetch_one(&pool)
            .await
            .expect("count rows");
    assert_eq!(count, 4, "the seeded row count must be unchanged");
}
