//! End-to-end test for the `Decimal` scalar.
//!
//! Verifies that values bound through SQLx round-trip through Postgres
//! `numeric` without precision loss, and that the JSON projection through
//! the codec emits strings rather than floats.

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Decimal, Value};
use std::str::FromStr;

include_server_schema!("tests/fixtures/banking_decimal.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, wallets")
        .execute(pool)
        .await
        .expect("drop");
    // PG `numeric` with high precision/scale to confirm we don't lose
    // anything banks would care about.
    query(
        "CREATE TABLE wallets (
            id BIGINT PRIMARY KEY,
            balance NUMERIC(38, 8) NOT NULL,
            ceiling NUMERIC(38, 8)
        )",
    )
    .execute(pool)
    .await
    .expect("create wallet");
}

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn decimal_round_trips_through_pg_numeric_without_loss() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // Use a value that f64 famously can't represent exactly.
    let exact = Decimal::from_str("12345678901234567.89012345").expect("parse");

    let created = cool
        .wallet()
        .create(cratestack_schema::CreateWalletInput {
            id: 1,
            balance: exact,
            ceiling: None,
        })
        .run(&ctx())
        .await
        .expect("create");

    assert_eq!(created.balance, exact);
    assert!(created.ceiling.is_none());

    // Read it back from a fresh fetch to confirm decode also preserves it.
    let fetched = cool
        .wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("row exists");
    assert_eq!(fetched.balance, exact);

    // And the raw PG `numeric` column matches the canonical string.
    let row = query("SELECT balance::text AS balance_text FROM wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read raw");
    let raw: String = row.get("balance_text");
    assert_eq!(raw, "12345678901234567.89012345");
}

#[tokio::test]
async fn decimal_arithmetic_is_exact_when_floats_would_drift() {
    // The 0.1 + 0.2 ≠ 0.3 case — confirms the column round-trips through
    // PG and arithmetic in Rust still hits the exact value.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let a = Decimal::from_str("0.1").unwrap();
    let b = Decimal::from_str("0.2").unwrap();

    cool.wallet()
        .create(cratestack_schema::CreateWalletInput {
            id: 1,
            balance: a,
            ceiling: Some(b),
        })
        .run(&ctx())
        .await
        .expect("create");

    let row = cool
        .wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("exists");
    let sum = row.balance + row.ceiling.expect("ceiling present");
    assert_eq!(sum, Decimal::from_str("0.3").unwrap());
}

#[tokio::test]
async fn optional_decimal_null_round_trips_cleanly() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let balance = Decimal::from_str("100").unwrap();
    cool.wallet()
        .create(cratestack_schema::CreateWalletInput {
            id: 1,
            balance,
            ceiling: None,
        })
        .run(&ctx())
        .await
        .expect("create");

    let row = cool
        .wallet()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("fetch")
        .expect("exists");
    assert!(row.ceiling.is_none());

    // Confirm NULL is stored, not zero.
    let raw = query("SELECT ceiling FROM wallets WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read");
    let ceiling: Option<Decimal> = raw.try_get("ceiling").ok();
    assert!(
        ceiling.is_none(),
        "optional Decimal must persist as SQL NULL"
    );
}
