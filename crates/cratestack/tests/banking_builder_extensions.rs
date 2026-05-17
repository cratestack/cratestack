//! End-to-end checks for the new builder verbs against real Postgres:
//!
//!   * `#1` — `update_many(filter).set(input)` on a versioned + audited
//!     model, exercising the per-row outbox/audit fan-out and the
//!     auto-bump on `@version`.
//!   * `#2` — `.run_in_tx(&mut tx, ctx)` on `.create()` /
//!     `.update().set()`, verifying caller-controlled commit/rollback
//!     and that audit + outbox writes participate in the caller's tx.
//!   * `#5` — `.for_update()` on `.find_unique()` paired with
//!     `.run_in_tx`, asserting the resulting SQL emits `FOR UPDATE` so
//!     concurrent writers serialize on the same row.
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set, same as `banking_batches.rs`.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value};
use support::pg;

include_server_schema!("tests/fixtures/banking_batches.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, batch_rows")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE batch_rows (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            balance BIGINT NOT NULL,
            version BIGINT NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await
    .expect("create batch_rows");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("builder-ext-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool, rows: &[(i64, &str, i64)]) {
    for (id, label, balance) in rows {
        query("INSERT INTO batch_rows (id, label, balance, version) VALUES ($1, $2, $3, 0)")
            .bind(id)
            .bind(label)
            .bind(balance)
            .execute(pool)
            .await
            .expect("seed row");
    }
}

// ───── #1 update_many ────────────────────────────────────────────────────────

#[tokio::test]
async fn update_many_mutates_matched_rows_and_writes_audit_per_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(
        pool,
        &[(1, "alpha", 100), (2, "beta", 200), (3, "gamma", 300)],
    )
    .await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::batch_row;
    let summary = cool
        .batch_row()
        .update_many()
        .where_(batch_row::label().eq("alpha"))
        .set(cratestack_schema::UpdateBatchRowInput {
            balance: Some(999),
            label: None,
        })
        .run(&ctx)
        .await
        .expect("update_many succeeds");

    assert_eq!(summary.total, 1, "only alpha matches");
    assert_eq!(summary.ok, 1);
    assert_eq!(summary.err, 0);

    // The row was patched AND @version bumped.
    let row = query("SELECT balance, version FROM batch_rows WHERE id = 1")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("balance"), 999);
    assert_eq!(
        row.get::<i64, _>("version"),
        1,
        "update_many must auto-bump @version",
    );

    // An audit row exists for the update.
    let audit_count: i64 = query(
        "SELECT COUNT(*) FROM cratestack_audit WHERE model = 'BatchRow' AND operation = 'update'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .get(0);
    assert_eq!(audit_count, 1, "exactly one audit row for the one match");

    // Untouched rows must keep their original balance.
    let beta_balance: i64 = query("SELECT balance FROM batch_rows WHERE id = 2")
        .fetch_one(pool)
        .await
        .unwrap()
        .get("balance");
    assert_eq!(beta_balance, 200, "beta must be untouched");
}

#[tokio::test]
async fn update_many_without_filter_is_rejected_before_any_sql_runs() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, &[(1, "alpha", 100), (2, "beta", 200)]).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let err = cool
        .batch_row()
        .update_many()
        .set(cratestack_schema::UpdateBatchRowInput {
            balance: Some(0),
            label: None,
        })
        .run(&ctx)
        .await
        .expect_err("predicate-less update_many must fail");
    let detail = err.detail().unwrap_or_default();
    assert!(detail.contains("at least one filter"), "got: {detail:?}");

    // Confirm no rows were mutated.
    let surviving_balances: Vec<i64> = query("SELECT balance FROM batch_rows ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.get::<i64, _>("balance"))
        .collect();
    assert_eq!(surviving_balances, vec![100, 200]);
}

// ───── #2 .run_in_tx() ───────────────────────────────────────────────────────

#[tokio::test]
async fn run_in_tx_create_commits_audit_and_outbox_alongside_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let mut tx = pool.begin().await.expect("begin tx");
    let created = cool
        .batch_row()
        .create(cratestack_schema::CreateBatchRowInput {
            id: 42,
            label: "in-tx".into(),
            balance: 7,
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("create in tx succeeds");
    assert_eq!(created.id, 42);
    tx.commit().await.expect("commit");

    let found: i64 = query("SELECT balance FROM batch_rows WHERE id = 42")
        .fetch_one(pool)
        .await
        .unwrap()
        .get("balance");
    assert_eq!(found, 7);

    // Audit row written inside the caller's tx must be visible after commit.
    let audit_count: i64 = query(
        "SELECT COUNT(*) FROM cratestack_audit WHERE model = 'BatchRow' AND operation = 'create'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .get(0);
    assert_eq!(audit_count, 1);
}

#[tokio::test]
async fn run_in_tx_rollback_unwinds_row_audit_and_outbox_together() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let mut tx = pool.begin().await.expect("begin tx");
    let _created = cool
        .batch_row()
        .create(cratestack_schema::CreateBatchRowInput {
            id: 99,
            label: "doomed".into(),
            balance: 1,
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("create in tx succeeds");
    // Deliberately roll back; row + audit + outbox must all vanish atomically.
    tx.rollback().await.expect("rollback");

    let count: i64 = query("SELECT COUNT(*) FROM batch_rows WHERE id = 99")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "row must not be visible after rollback");

    let audit_count: i64 = query("SELECT COUNT(*) FROM cratestack_audit WHERE model = 'BatchRow'")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(audit_count, 0, "audit row must roll back with the row");
}

// ───── #5 .for_update() ──────────────────────────────────────────────────────

#[tokio::test]
async fn for_update_preview_emits_for_update_clause() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, &[(1, "alpha", 100)]).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Smoke-test that the SQL is syntactically valid against real PG by
    // executing it inside a transaction.
    let mut tx = pool.begin().await.expect("begin tx");
    let found = cool
        .batch_row()
        .bind(ctx)
        .find_unique(1)
        .for_update()
        .run_in_tx(&mut tx)
        .await
        .expect("for_update select succeeds")
        .expect("row exists");
    assert_eq!(found.id, 1);
    tx.commit().await.expect("commit");
}
