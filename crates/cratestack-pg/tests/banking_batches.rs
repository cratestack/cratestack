//! End-to-end test for the five batch primitives against a real Postgres.
//!
//! The tests in this file exercise the envelope contract corners that the
//! sqlite_e2e tests can't reach: per-item audit rows, per-item event
//! outbox entries, savepoint rollback under `if_match` mismatch on a
//! versioned model, and that successful items in the same batch commit
//! atomically with their audit + outbox writes.
//!
//! Proof-of-concept caller for `tests/support/pg.rs`: this is the first
//! `banking_*.rs` file migrated to the centralised PG-backend selector.
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{BatchItemStatus, CoolContext, Value};
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
        .with_request_id("batch-trace-id-001")
}

fn ok_value(
    item: &cratestack::BatchItemResult<cratestack_schema::BatchRow>,
) -> &cratestack_schema::BatchRow {
    match &item.status {
        BatchItemStatus::Ok { value } => value,
        BatchItemStatus::Error { error } => {
            panic!("expected Ok at index {}, got Err({:?})", item.index, error)
        }
    }
}

fn err_code(item: &cratestack::BatchItemResult<cratestack_schema::BatchRow>) -> &str {
    match &item.status {
        BatchItemStatus::Error { error } => error.code.as_str(),
        BatchItemStatus::Ok { .. } => {
            panic!("expected Err at index {}, got Ok", item.index)
        }
    }
}

#[tokio::test]
async fn batch_create_writes_one_audit_row_per_successful_item() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let response = cool
        .batch_row()
        .batch_create(vec![
            cratestack_schema::CreateBatchRowInput {
                id: 1,
                label: "alpha".into(),
                balance: 100,
            },
            cratestack_schema::CreateBatchRowInput {
                id: 2,
                label: "beta".into(),
                balance: 200,
            },
        ])
        .run(&ctx)
        .await
        .expect("batch_create infra ok");

    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 0);

    // Two audit rows — one per successful item — with the same request id.
    let audit_count: i64 =
        query("SELECT COUNT(*)::BIGINT FROM cratestack_audit WHERE model = 'BatchRow'")
            .fetch_one(pool)
            .await
            .expect("count audit rows")
            .get(0);
    assert_eq!(audit_count, 2);

    let request_ids: Vec<String> =
        query("SELECT request_id FROM cratestack_audit ORDER BY occurred_at")
            .fetch_all(pool)
            .await
            .expect("fetch audit request ids")
            .into_iter()
            .map(|row| row.get::<String, _>("request_id"))
            .collect();
    assert!(
        request_ids.iter().all(|id| id == "batch-trace-id-001"),
        "all audit rows must carry the batch's request id, got: {request_ids:?}",
    );

    // And one outbox entry per item.
    let outbox_count: i64 =
        query("SELECT COUNT(*)::BIGINT FROM cratestack_event_outbox WHERE model = 'BatchRow'")
            .fetch_one(pool)
            .await
            .expect("count outbox rows")
            .get(0);
    assert_eq!(outbox_count, 2);
}

#[tokio::test]
async fn batch_update_with_stale_if_match_rolls_back_only_that_item() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Seed two rows. Both start at version 0.
    cool.batch_row()
        .batch_create(vec![
            cratestack_schema::CreateBatchRowInput {
                id: 10,
                label: "ten".into(),
                balance: 10,
            },
            cratestack_schema::CreateBatchRowInput {
                id: 20,
                label: "twenty".into(),
                balance: 20,
            },
        ])
        .run(&ctx)
        .await
        .expect("seed");

    // Update item 10 from version 0 (fresh) — success.
    // Update item 20 from version 99 (stale) — PRECONDITION_FAILED, rolls
    // back the savepoint, leaves item 20 untouched.
    let response = cool
        .batch_row()
        .batch_update(vec![
            (
                10,
                cratestack_schema::UpdateBatchRowInput {
                    label: Some("ten-updated".into()),
                    balance: Some(11),
                },
                Some(0),
            ),
            (
                20,
                cratestack_schema::UpdateBatchRowInput {
                    label: Some("twenty-NEVER".into()),
                    balance: Some(99),
                },
                Some(99),
            ),
        ])
        .run(&ctx)
        .await
        .expect("batch_update infra ok despite per-item failure");

    assert_eq!(response.summary.ok, 1);
    assert_eq!(response.summary.err, 1);
    assert_eq!(ok_value(&response.results[0]).label, "ten-updated");
    assert_eq!(err_code(&response.results[1]), "PRECONDITION_FAILED");

    // Item 10 committed AND bumped version. Item 20 must be exactly as
    // seeded — no balance change, no version bump.
    let row_10: (String, i64, i64) =
        query("SELECT label, balance, version FROM batch_rows WHERE id = 10")
            .fetch_one(pool)
            .await
            .expect("fetch row 10")
            .try_into_pair();
    assert_eq!(row_10.0, "ten-updated");
    assert_eq!(row_10.1, 11);
    assert_eq!(row_10.2, 1, "successful item bumped version");

    let row_20: (String, i64, i64) =
        query("SELECT label, balance, version FROM batch_rows WHERE id = 20")
            .fetch_one(pool)
            .await
            .expect("fetch row 20")
            .try_into_pair();
    assert_eq!(row_20.0, "twenty");
    assert_eq!(row_20.1, 20);
    assert_eq!(row_20.2, 0, "failed item must NOT have bumped version");
}

#[tokio::test]
async fn batch_upsert_mixes_create_and_update_audit_operations() {
    // batch_upsert per item drives the SELECT FOR UPDATE probe → INSERT
    // ON CONFLICT flow. Each item's audit row should be tagged with the
    // operation that actually fired (`create` for new inserts,
    // `update` for ON CONFLICT branch).
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Seed one row so the upsert can hit both branches.
    cool.batch_row()
        .create(cratestack_schema::CreateBatchRowInput {
            id: 100,
            label: "existing".into(),
            balance: 0,
        })
        .run(&ctx)
        .await
        .expect("seed existing row");

    let response = cool
        .batch_row()
        .batch_upsert(vec![
            // INSERT branch — new id.
            cratestack_schema::CreateBatchRowInput {
                id: 200,
                label: "newly-inserted".into(),
                balance: 1,
            },
            // UPDATE branch — collides with the seeded row.
            cratestack_schema::CreateBatchRowInput {
                id: 100,
                label: "newly-updated".into(),
                balance: 2,
            },
        ])
        .run(&ctx)
        .await
        .expect("batch_upsert infra ok");

    assert_eq!(response.summary.ok, 2);

    // Audit should record one `create` and one `update` — both for the
    // batch_upsert, but with the right operation tag per row.
    //
    // Note: the seed `.create()` above also wrote an audit row, so there
    // are three audit rows total. We filter to the ones triggered by the
    // upsert batch.
    let ops: Vec<String> = query(
        "SELECT operation FROM cratestack_audit
         WHERE primary_key = '200'::jsonb OR (primary_key = '100'::jsonb AND occurred_at = (
             SELECT MAX(occurred_at) FROM cratestack_audit WHERE primary_key = '100'::jsonb
         ))
         ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("fetch upsert audit ops")
    .into_iter()
    .map(|row| row.get::<String, _>("operation"))
    .collect();
    assert!(
        ops.contains(&"create".to_owned()) && ops.contains(&"update".to_owned()),
        "upsert audit should cover both branches, got: {ops:?}",
    );
}

#[tokio::test]
async fn batch_delete_writes_audit_before_snapshot_from_returning_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.batch_row()
        .batch_create(vec![
            cratestack_schema::CreateBatchRowInput {
                id: 300,
                label: "doomed-a".into(),
                balance: 1,
            },
            cratestack_schema::CreateBatchRowInput {
                id: 400,
                label: "doomed-b".into(),
                balance: 2,
            },
        ])
        .run(&ctx)
        .await
        .expect("seed");

    let response = cool
        .batch_row()
        .batch_delete(vec![300, 400, 999])
        .run(&ctx)
        .await
        .expect("batch_delete infra ok");

    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 1);
    assert_eq!(err_code(&response.results[2]), "NOT_FOUND");

    // The delete-audit rows must carry the BEFORE snapshot (the pre-
    // deletion row state captured from RETURNING). Two delete-audit rows
    // expected, one per deleted item; each `before` carries the original
    // label.
    let delete_audit: Vec<(String, serde_json::Value)> = query(
        "SELECT operation, before
         FROM cratestack_audit
         WHERE operation = 'delete'
         ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("fetch delete audit rows")
    .into_iter()
    .map(|row| {
        (
            row.get::<String, _>("operation"),
            row.get::<serde_json::Value, _>("before"),
        )
    })
    .collect();
    assert_eq!(delete_audit.len(), 2);
    let labels: Vec<&str> = delete_audit
        .iter()
        .filter_map(|(_, b)| b.get("label").and_then(|v| v.as_str()))
        .collect();
    assert!(labels.contains(&"doomed-a"));
    assert!(labels.contains(&"doomed-b"));
}

#[tokio::test]
async fn batch_duplicate_pk_rejects_before_any_audit_or_outbox_write() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let err = cool
        .batch_row()
        .batch_update(vec![
            (
                500,
                cratestack_schema::UpdateBatchRowInput {
                    label: Some("first".into()),
                    balance: None,
                },
                Some(0),
            ),
            (
                500, // duplicate
                cratestack_schema::UpdateBatchRowInput {
                    label: Some("second".into()),
                    balance: None,
                },
                Some(0),
            ),
        ])
        .run(&ctx)
        .await
        .expect_err("dup PK loud-fails");

    assert_eq!(err.code(), "VALIDATION_ERROR");

    // No audit or outbox writes — the guard fires before any tx work.
    let audit_count: i64 = query("SELECT COUNT(*)::BIGINT FROM cratestack_audit")
        .fetch_one(pool)
        .await
        .map(|row| row.get(0))
        .unwrap_or(0);
    assert_eq!(audit_count, 0);
    let outbox_count: i64 = query("SELECT COUNT(*)::BIGINT FROM cratestack_event_outbox")
        .fetch_one(pool)
        .await
        .map(|row| row.get(0))
        .unwrap_or(0);
    assert_eq!(outbox_count, 0);
}

// Tiny conversion helper that lets us spell out a row's columns in a
// concise tuple while keeping the test bodies readable.
trait IntoPair<T> {
    fn try_into_pair(self) -> T;
}
impl IntoPair<(String, i64, i64)> for cratestack::sqlx::postgres::PgRow {
    fn try_into_pair(self) -> (String, i64, i64) {
        (self.get(0), self.get(1), self.get(2))
    }
}
