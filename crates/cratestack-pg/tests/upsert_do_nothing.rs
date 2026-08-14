//! `.upsert(..).on_conflict(..).do_nothing()` (cratestack#487, ADR 0038
//! blocker B3).
//!
//! `upsert_do_nothing_ledger_corruption_regression` is the acceptance
//! test: it reproduces the exact bug from the issue (a cash-in claim
//! retry silently overwriting a completed ledger row via `DO UPDATE`)
//! and asserts `.do_nothing()` refuses to let that happen. Confirmed
//! against `main` (028cdc5) via a throwaway reproduction using today's
//! only available `.upsert().run()` (DO UPDATE) path: retrying with
//! blank values *does* overwrite the original row's `payload` there —
//! see the PR description for the exact command/output. That exact
//! test cannot run unmodified against `main` because `.do_nothing()`
//! does not exist there; the throwaway repro is the closest exact
//! equivalent (same corruption, same mechanism, current-only API).
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set (see `tests/support/pg.rs`).

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CratestackContext, UpsertOutcome, Value};
use support::pg;

include_server_schema!("tests/fixtures/upsert_do_nothing.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query(
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, \
         cash_in_claims, idempotent_markers",
    )
    .execute(pool)
    .await
    .expect("drop tables");
    query(
        "CREATE TABLE cash_in_claims (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            transfer_ref TEXT NOT NULL,
            new_balance_xaf BIGINT NOT NULL,
            completed_at TEXT
        )",
    )
    .execute(pool)
    .await
    .expect("create cash_in_claims");
    query(
        "CREATE TABLE idempotent_markers (
            id TEXT PRIMARY KEY,
            version BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create idempotent_markers");
}

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("issue-487")
}

fn completed_claim(id: &str) -> cratestack_schema::CreateCashInClaimInput {
    cratestack_schema::CreateCashInClaimInput {
        id: id.to_owned(),
        status: "COMPLETED".into(),
        transferRef: "transfer-ref-original".into(),
        newBalanceXaf: 5_000,
        completedAt: Some("2026-08-09T00:00:00Z".into()),
    }
}

fn retry_with_blank_values(id: &str) -> cratestack_schema::CreateCashInClaimInput {
    cratestack_schema::CreateCashInClaimInput {
        id: id.to_owned(),
        status: "PENDING".into(),
        transferRef: String::new(),
        newBalanceXaf: 0,
        completedAt: None,
    }
}

// ───── #1 the acceptance-bar regression test ─────────────────────────────

#[tokio::test]
async fn upsert_do_nothing_ledger_corruption_regression() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Original attempt: a completed cash-in claim with meaningful
    // transfer_ref / new_balance_xaf / completed_at.
    let first = cool
        .cash_in_claim()
        .upsert(completed_claim("claim-42"))
        .do_nothing()
        .run(&ctx)
        .await
        .expect("first attempt inserts");
    let UpsertOutcome::Inserted(inserted) = first else {
        panic!("first attempt on an empty table must be Inserted, got {first:?}");
    };
    assert_eq!(inserted.status, "COMPLETED");
    assert_eq!(inserted.transferRef, "transfer-ref-original");
    assert_eq!(inserted.newBalanceXaf, 5_000);

    // Retry of the *same logical claim* (e.g. a client retrying after
    // a timeout) with blank/default values, because the retry never
    // learned the real transfer_ref/balance.
    let second = cool
        .cash_in_claim()
        .upsert(retry_with_blank_values("claim-42"))
        .do_nothing()
        .run(&ctx)
        .await
        .expect("retry must not error");
    let UpsertOutcome::Existing(existing) = second else {
        panic!("retry against an existing row must be Existing, got {second:?}");
    };

    // THE REGRESSION ASSERTION: the row `.do_nothing()` reports back
    // is the ORIGINAL data, completely unmodified by the retry's blank
    // values. This is the exact assertion that fails under `main`'s
    // only available `.upsert().run()` path (DO UPDATE) — see the
    // module doc comment for the confirmed-on-main repro.
    assert_eq!(existing.status, "COMPLETED", "status must not be reset");
    assert_eq!(
        existing.transferRef, "transfer-ref-original",
        "transfer_ref must not be blanked by the retry"
    );
    assert_eq!(
        existing.newBalanceXaf, 5_000,
        "new_balance_xaf must not be zeroed by the retry"
    );
    assert_eq!(
        existing.completedAt,
        Some("2026-08-09T00:00:00Z".to_owned()),
        "completed_at must not be cleared by the retry"
    );

    // Re-read directly from the table too, independent of what the
    // builder returned, to rule out an in-memory-only guarantee.
    let row =
        query("SELECT status, transfer_ref, new_balance_xaf FROM cash_in_claims WHERE id = $1")
            .bind("claim-42")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(row.get::<String, _>("status"), "COMPLETED");
    assert_eq!(
        row.get::<String, _>("transfer_ref"),
        "transfer-ref-original"
    );
    assert_eq!(row.get::<i64, _>("new_balance_xaf"), 5_000);
    let count: i64 = query("SELECT COUNT(*) FROM cash_in_claims")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1, "the retry must not have inserted a second row");
}

// ───── #2 Inserted vs Existing is distinguishable in both directions ─────

#[tokio::test]
async fn upsert_do_nothing_distinguishes_inserted_from_existing() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let first = cool
        .cash_in_claim()
        .upsert(completed_claim("claim-1"))
        .do_nothing()
        .run(&ctx)
        .await
        .expect("insert branch");
    assert!(first.was_inserted(), "first call must report Inserted");

    let second = cool
        .cash_in_claim()
        .upsert(retry_with_blank_values("claim-1"))
        .do_nothing()
        .run(&ctx)
        .await
        .expect("conflict branch");
    assert!(
        !second.was_inserted(),
        "second call on the same id must report Existing, not Inserted"
    );

    // No `Updated` event and no audit entry: DO NOTHING never mutated
    // anything, so there is nothing to report as changed.
    let audit_ops: Vec<String> = query(
        "SELECT operation FROM cratestack_audit WHERE model = 'CashInClaim' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.get::<String, _>("operation"))
    .collect();
    assert_eq!(
        audit_ops,
        vec!["create".to_string()],
        "do_nothing's Existing branch must not write an audit entry"
    );
    let event_ops: Vec<String> = query(
        "SELECT operation FROM cratestack_event_outbox WHERE model = 'CashInClaim' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.get::<String, _>("operation"))
    .collect();
    assert_eq!(
        event_ops,
        vec!["created".to_string()],
        "do_nothing's Existing branch must not enqueue an Updated event"
    );
}

// ───── #3 the existing DO UPDATE upsert path is unchanged ───────────────

#[tokio::test]
async fn plain_upsert_do_update_path_still_merges_on_conflict() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.cash_in_claim()
        .upsert(completed_claim("claim-99"))
        .run(&ctx)
        .await
        .expect("plain upsert insert branch");

    // The plain (non-`do_nothing`) `.upsert().run()` path must behave
    // exactly as it did before #487: DO UPDATE merges the new values
    // into the conflicting row.
    let merged = cool
        .cash_in_claim()
        .upsert(retry_with_blank_values("claim-99"))
        .run(&ctx)
        .await
        .expect("plain upsert update branch");
    assert_eq!(
        merged.status, "PENDING",
        "DO UPDATE path must still overwrite on conflict"
    );
    assert_eq!(merged.transferRef, "");

    let audit_ops: Vec<String> = query(
        "SELECT operation FROM cratestack_audit WHERE model = 'CashInClaim' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.get::<String, _>("operation"))
    .collect();
    assert_eq!(
        audit_ops,
        vec!["create".to_string(), "update".to_string()],
        "DO UPDATE path must still audit both operations, unaffected by do_nothing existing"
    );
}

// ───── #4 empty upsert_update_columns model ──────────────────────────────

#[tokio::test]
async fn do_nothing_works_on_a_model_with_empty_upsert_update_columns() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // `IdempotentMarker` has zero eligible `upsert_update_columns`
    // (only `id` and `@version`) — the model-descriptor-driven no-op
    // self-assignment fallback in the DO UPDATE path exists for this
    // shape. `.do_nothing()` doesn't consult `upsert_update_columns`
    // at all, so it must work identically here.
    let first = cool
        .idempotent_marker()
        .upsert(cratestack_schema::CreateIdempotentMarkerInput {
            id: "marker-1".into(),
        })
        .do_nothing()
        .run(&ctx)
        .await
        .expect("insert branch on empty-upsert_update_columns model");
    assert!(first.was_inserted());
    assert_eq!(first.record().version, 0);

    let second = cool
        .idempotent_marker()
        .upsert(cratestack_schema::CreateIdempotentMarkerInput {
            id: "marker-1".into(),
        })
        .do_nothing()
        .run(&ctx)
        .await
        .expect("conflict branch on empty-upsert_update_columns model");
    assert!(!second.was_inserted());
    // DO NOTHING never touches the row, so `@version` is NOT bumped —
    // unlike the DO UPDATE path's `<col> = <col> + 1` version clause.
    assert_eq!(
        second.record().version,
        0,
        "do_nothing must not bump @version on the existing row"
    );

    let count: i64 = query("SELECT COUNT(*) FROM idempotent_markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
}
