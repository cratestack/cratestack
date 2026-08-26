//! `ConflictTarget::where_index` (cratestack#741) against a real
//! **partial** unique index — `CREATE UNIQUE INDEX ... WHERE ...`,
//! created outside the schema (declaring one in schema DDL is
//! cratestack#742, out of scope here).
//!
//! `probe_is_predicate_aware_for_a_non_null_test_predicate` is the
//! acceptance-bar regression test. It is deliberately NOT a `col IS
//! NOT NULL` predicate: that shape happens to give the right answer
//! from a predicate-UNAWARE probe too, because SQL's `col = NULL`
//! never matches any row — see its doc comment. Manually confirmed
//! FAILING (`Existing(Marker { id: 1, k: Some("shared-key"), status:
//! "active", ... })` returned where `Inserted` was expected) with
//! `select_for_update_by_conflict_target`'s `AND (<predicate>)` clause
//! temporarily stubbed out to `let _ = predicate;` — i.e. exactly the
//! pre-cratestack#741 column-only probe with the emitted SQL otherwise
//! unchanged — then confirmed PASSING again with the stub reverted.
//! See the PR description for the captured failure output.
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set (see `tests/support/pg.rs`).

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{ConflictTarget, CratestackContext, UpsertOutcome, Value};
use support::pg;

include_server_schema!("tests/fixtures/upsert_partial_index.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, markers")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE markers (
            id BIGINT PRIMARY KEY,
            k TEXT,
            status TEXT NOT NULL,
            payload TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create markers");
    // The partial unique index this whole test file exists to prove
    // `.on_conflict(...)` can target. NOT declared in the schema
    // (cratestack#742 is the separate ticket for that) — created
    // directly here, exactly like the webank consumer's index this
    // ticket was filed for.
    query("CREATE UNIQUE INDEX idx_markers_active_k ON markers(k) WHERE status = 'active'")
        .execute(pool)
        .await
        .expect("create partial unique index");
}

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("issue-741")
}

fn marker(
    id: i64,
    k: Option<&str>,
    status: &str,
    payload: &str,
) -> cratestack_schema::CreateMarkerInput {
    cratestack_schema::CreateMarkerInput {
        id,
        k: k.map(str::to_owned),
        status: status.to_owned(),
        payload: payload.to_owned(),
    }
}

fn active_k_target() -> ConflictTarget {
    ConflictTarget::columns(&["k"]).where_index("status = 'active'")
}

// ───── #1 the acceptance-bar regression test ─────────────────────────────

#[tokio::test]
async fn probe_is_predicate_aware_for_a_non_null_test_predicate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // An `active` row with k='shared-key' exists — it's the only row
    // the partial index covers for that k.
    let active = cool
        .marker()
        .upsert(marker(1, Some("shared-key"), "active", "original"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("seed the active row");
    assert!(active.was_inserted());

    // Upsert an `archived` row with the SAME k. Postgres would INSERT
    // this (the partial index's uniqueness domain doesn't cover
    // `status = 'archived'` rows at all), so the caller must see
    // `Inserted` — not `Existing` from a probe that matched the active
    // row by filtering on `k` alone, ignoring the predicate.
    let archived = cool
        .marker()
        .upsert(marker(2, Some("shared-key"), "archived", "retry"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("archived upsert must not error");
    assert!(
        archived.was_inserted(),
        "an archived row with the same k as an active row is OUTSIDE the partial index's \
         uniqueness domain and must be a real INSERT, not treated as a conflict with the \
         active row: got {archived:?}",
    );
    assert_ne!(
        archived.record().id,
        active.record().id,
        "must be a distinct row"
    );

    // Both rows exist, independently, untouched by each other.
    let rows = query("SELECT id, status, payload FROM markers ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "both rows must exist");
    assert_eq!(rows[0].get::<String, _>("status"), "active");
    assert_eq!(rows[0].get::<String, _>("payload"), "original");
    assert_eq!(rows[1].get::<String, _>("status"), "archived");
    assert_eq!(rows[1].get::<String, _>("payload"), "retry");
}

// ───── #2 a real conflict WITHIN the predicate still resolves Existing ──

#[tokio::test]
async fn probe_still_returns_existing_for_a_conflict_within_the_predicate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let first = cool
        .marker()
        .upsert(marker(1, Some("dup-key"), "active", "v1"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("first insert");
    assert!(first.was_inserted());

    // A second `active` row with the SAME k genuinely conflicts — it
    // IS inside the partial index's domain — so this must resolve
    // `Existing`, and the row must be untouched (DO NOTHING semantics).
    let second = cool
        .marker()
        .upsert(marker(2, Some("dup-key"), "active", "v2-should-be-dropped"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("second upsert must not error");
    assert!(!second.was_inserted(), "must resolve Existing: {second:?}");
    let UpsertOutcome::Existing(existing) = second else {
        unreachable!()
    };
    assert_eq!(existing.id, first.record().id);
    assert_eq!(existing.payload, "v1", "DO NOTHING must not overwrite");

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
}

// ───── #3 NULL k is outside the predicate on both calls ──────────────────

#[tokio::test]
async fn null_key_is_always_outside_the_predicate_both_calls_insert() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let first = cool
        .marker()
        .upsert(marker(1, None, "active", "a"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("first NULL-k insert");
    let second = cool
        .marker()
        .upsert(marker(2, None, "active", "b"))
        .on_conflict(active_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("second NULL-k insert");

    assert!(first.was_inserted());
    assert!(
        second.was_inserted(),
        "NULL k is outside the index predicate, so no conflict is possible: {second:?}",
    );
    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 2);
}

// ───── #4 the DO UPDATE path gets the same treatment ──────────────────────

#[tokio::test]
async fn do_update_path_treats_out_of_predicate_row_as_a_real_insert() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.marker()
        .upsert(marker(1, Some("k"), "active", "original"))
        .on_conflict(active_k_target())
        .run(&ctx)
        .await
        .expect("seed active row via the DO UPDATE path");

    // Plain `.upsert().run()` (DO UPDATE, not `.do_nothing()`) on an
    // archived row with the same k must INSERT a second row, not
    // merge into the active one — the DO UPDATE path's `before_record`
    // probe has to be predicate-aware for the same reason the
    // `.do_nothing()` probe does (cratestack#741).
    let archived = cool
        .marker()
        .upsert(marker(2, Some("k"), "archived", "retry"))
        .on_conflict(active_k_target())
        .run(&ctx)
        .await
        .expect("archived upsert via DO UPDATE path");
    assert_eq!(archived.payload, "retry");
    assert_eq!(
        archived.id, 2,
        "must be its own row, not id 1's merge target"
    );

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 2, "both rows must exist independently");

    // The audit trail must show two `create`s, not a `create` +
    // `update` — a mis-probed `before_record` would have wrongly
    // classified the second call as an update of row 1.
    let audit_ops: Vec<String> =
        query("SELECT operation FROM cratestack_audit WHERE model = 'Marker' ORDER BY occurred_at")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.get::<String, _>("operation"))
            .collect();
    assert_eq!(audit_ops, vec!["create".to_string(), "create".to_string()]);
}

// ───── #5 unpredicated conflict targets are unaffected ───────────────────

#[tokio::test]
async fn unpredicated_conflict_target_still_works_unchanged() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // No partial-index predicate at all — targets the model's PK, same
    // as every upsert before cratestack#741.
    let first = cool
        .marker()
        .upsert(marker(1, None, "active", "v1"))
        .run(&ctx)
        .await
        .expect("pk-conflict insert");
    let second = cool
        .marker()
        .upsert(marker(1, None, "active", "v2"))
        .run(&ctx)
        .await
        .expect("pk-conflict update");
    assert_eq!(first.id, second.id);
    assert_eq!(second.payload, "v2");

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
}

// ───── #6 predicate + PrimaryKey is a clear error ─────────────────────────

#[tokio::test]
async fn predicate_on_primary_key_is_rejected_before_any_sql_runs() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let bad_target = ConflictTarget::PRIMARY_KEY.where_index("status = 'active'");
    let err = cool
        .marker()
        .upsert(marker(1, Some("x"), "active", "v1"))
        .on_conflict(bad_target)
        .run(&ctx)
        .await
        .expect_err("PK + predicate must be rejected, not silently dropped");
    let detail = err.detail().unwrap_or_default();
    assert!(
        detail.contains("primary key"),
        "error should explain why, got: {detail:?}",
    );

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "the rejected call must not have run any SQL");
}
