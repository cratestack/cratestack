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
use cratestack::{CratestackContext, Decimal, Value};
use std::str::FromStr;

include_server_schema!(
    "tests/fixtures/decimal_bigdecimal_backend.cstack",
    db = Postgres,
    decimal = BigDecimal
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, big_decimal_wallets")
        .execute(pool)
        .await
        .expect("drop");
    // NUMERIC(60, 20) (not NUMERIC(38, 8)): wide enough to hold
    // `decimal_round_trips_beyond_rust_decimal_capacity_under_bigdecimal_backend`'s
    // 40-significant-digit value below, which is deliberately beyond what
    // `rust_decimal::Decimal`'s 96-bit mantissa can represent at all (~28-29
    // significant digits) — see that test's own doc comment. Still holds the
    // narrower in-range values the other two tests use.
    query(
        "CREATE TABLE big_decimal_wallets (
            id BIGINT PRIMARY KEY,
            balance NUMERIC(60, 20) NOT NULL,
            ceiling NUMERIC(60, 20)
        )",
    )
    .execute(pool)
    .await
    .expect("create big_decimal_wallets");
}

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
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
    // feature), not just round-tripped through in-process state. The column
    // is `NUMERIC(60, 20)` (see `reset_schema`), so Postgres right-pads the
    // fractional part to 20 digits — the comparison accounts for that
    // instead of asserting the bare input string back.
    let row = query("SELECT balance::text AS balance_text FROM big_decimal_wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read raw");
    let raw: String = row.get("balance_text");
    assert_eq!(raw, "12345678901234567.89012345000000000000");
}

/// The other round-trip test above uses a 25-significant-digit value
/// (`12345678901234567.89012345`), which sits comfortably *within*
/// `rust_decimal`'s ~28-29 significant-digit capacity — it never actually
/// exercises what `decimal-bigdecimal` exists for (cratestack#496 review
/// finding). This test uses a 40-significant-digit value instead
/// (30 integer digits + 10 fractional digits).
///
/// Verified out-of-band (not asserted in-process: this file is compiled
/// with `--no-default-features --features postgres,decimal-bigdecimal`,
/// under which `rust_decimal` is not in the dependency graph at all —
/// pulling it in here just for a sanity check would defeat the "no
/// `rust_decimal` anywhere in the graph" acceptance bar `.ci/feature-
/// matrix.sh` asserts for this exact combination) —
/// `"123456789012345678901234567890.1234567890".parse::<rust_decimal::
/// Decimal>()` returns `Err(... "overflow from too many digits")` on the
/// default backend. This value is not just imprecise under
/// `decimal-rust-decimal`, it is **unrepresentable**. Round-tripping it
/// exactly through Postgres `NUMERIC` under `decimal-bigdecimal` is the
/// actual acceptance bar for this feature existing at all.
#[tokio::test]
async fn decimal_round_trips_beyond_rust_decimal_capacity_under_bigdecimal_backend() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let beyond_capacity =
        Decimal::from_str("123456789012345678901234567890.1234567890").expect("parse");

    let created = cool
        .big_decimal_wallet()
        .create(cratestack_schema::CreateBigDecimalWalletInput {
            id: 1,
            balance: beyond_capacity.clone(),
            ceiling: Some(beyond_capacity.clone()),
        })
        .run(&ctx())
        .await
        .expect("create");

    assert_eq!(created.balance, beyond_capacity);
    assert_eq!(created.ceiling, Some(beyond_capacity.clone()));

    let fetched = cool
        .big_decimal_wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("row exists");
    assert_eq!(fetched.balance, beyond_capacity);
    assert_eq!(fetched.ceiling, Some(beyond_capacity));

    // Raw PG text confirms the full 40-digit value actually reached
    // Postgres unrounded, not just round-tripped through in-process state.
    let row = query("SELECT balance::text AS balance_text FROM big_decimal_wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read raw");
    let raw: String = row.get("balance_text");
    assert_eq!(raw, "123456789012345678901234567890.12345678900000000000");
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
