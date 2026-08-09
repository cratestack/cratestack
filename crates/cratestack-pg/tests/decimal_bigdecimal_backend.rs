//! End-to-end check that a `Decimal`-typed field round-trips through
//! Postgres `numeric` under the `decimal-bigdecimal` backend
//! (cratestack#495) — the `bigdecimal`-flavored counterpart to
//! `banking_decimal.rs`'s `decimal_round_trips_through_pg_numeric_without_loss`,
//! which only ever runs against the default `decimal-rust-decimal`
//! backend.
//!
//! Gated `required-features = ["postgres", "decimal-bigdecimal"]` in
//! `Cargo.toml`, mirroring `pgvector_feature_forwarding.rs`'s pattern —
//! this file only compiles under `cargo test -p cratestack-pg
//! --no-default-features --features postgres,decimal-bigdecimal`. The
//! `--no-default-features` is load-bearing, not optional convenience: this
//! crate's `default` feature set includes `decimal-rust-decimal`, and the
//! two backends are mutually exclusive (`cratestack-core`'s
//! `compile_error!` — see that crate's `src/lib.rs`), so
//! `--features decimal-bigdecimal` alone, with defaults still active,
//! fails to compile the whole crate rather than just skipping this file.
//!
//! Like every other `banking_*`/`*_decimal` PG-backed test, this skips
//! silently without `CRATESTACK_TEST_DATABASE_URL` (or
//! `CRATESTACK_USE_TESTCONTAINERS=1`) set — see `tests/support/pg.rs`.
//!
//! Uses its own `BigDecimalWallet` model / `big_decimal_wallets` table
//! (not `banking_decimal.rs`'s `Wallet`/`wallets`) — `cargo test -p
//! cratestack-pg` runs every file in this directory as its own OS process
//! against the same shared Postgres, and `fixture_table_names.rs`
//! statically rejects two *different* fixtures landing on the same
//! default table name (see that file's module doc).

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Decimal, Value};
use std::str::FromStr;

include_server_schema!(
    "tests/fixtures/decimal_bigdecimal_backend.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, big_decimal_wallets")
        .execute(pool)
        .await
        .expect("drop");
    query(
        "CREATE TABLE big_decimal_wallets (
            id BIGINT PRIMARY KEY,
            balance NUMERIC(38, 8) NOT NULL,
            ceiling NUMERIC(38, 8)
        )",
    )
    .execute(pool)
    .await
    .expect("create big_decimal_wallets");
}

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn decimal_round_trips_through_pg_numeric_under_bigdecimal_backend() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // Under `decimal-rust-decimal`, `Decimal` is `Copy`, so `banking_decimal.rs`
    // can read `exact` after moving it into `CreateWalletInput`. Under
    // `decimal-bigdecimal`, `Decimal` (== `bigdecimal::BigDecimal`) is NOT
    // `Copy` — it heap-allocates its digit buffer — so every reuse below is
    // an explicit `.clone()`. This is the exact call-site change
    // cratestack#495's audit flagged as the backend swap's real risk: code
    // that relied on implicit `Decimal` copies breaks without it.
    let exact = Decimal::from_str("12345678901234567.89012345").expect("parse");

    let created = cool
        .big_decimal_wallet()
        .create(cratestack_schema::CreateBigDecimalWalletInput {
            id: 1,
            balance: exact.clone(),
            ceiling: None,
        })
        .run(&ctx())
        .await
        .expect("create");

    assert_eq!(created.balance, exact);
    assert!(created.ceiling.is_none());

    // Read it back from a fresh fetch to confirm decode also preserves it.
    let fetched = cool
        .big_decimal_wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("row exists");
    assert_eq!(fetched.balance, exact);

    // And the raw PG `numeric` column matches the canonical string — proves
    // the value actually reached Postgres via `sqlx-postgres`'s `bigdecimal`
    // `Encode`/`Type` impls (`cratestack-sqlx`'s `decimal-bigdecimal`
    // feature), not just round-tripped through in-process state.
    let row = query("SELECT balance::text AS balance_text FROM big_decimal_wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read raw");
    let raw: String = row.get("balance_text");
    assert_eq!(raw, "12345678901234567.89012345");
}

#[tokio::test]
async fn optional_decimal_null_round_trips_cleanly_under_bigdecimal_backend() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let balance = Decimal::from_str("100").unwrap();
    cool.big_decimal_wallet()
        .create(cratestack_schema::CreateBigDecimalWalletInput {
            id: 1,
            balance,
            ceiling: None,
        })
        .run(&ctx())
        .await
        .expect("create");

    let row = cool
        .big_decimal_wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("exists");
    assert!(row.ceiling.is_none());

    // Confirm NULL is stored, not zero.
    let raw = query("SELECT ceiling FROM big_decimal_wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read");
    let ceiling: Option<Decimal> = raw.try_get("ceiling").ok();
    assert!(
        ceiling.is_none(),
        "optional Decimal must persist as SQL NULL under decimal-bigdecimal too"
    );
}
