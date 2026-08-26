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
            payload TEXT NOT NULL,
            flag TEXT,
            mode TEXT NOT NULL DEFAULT 'expedited'
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

/// cratestack#741 finding 1: `flag` is nullable with no default, so
/// `flag = 'yes'` hits SQL's three-valued logic when `flag` is `NULL`
/// — the predicate result is `NULL`, not `false`. A separate helper
/// (not part of `reset_schema`) so it only exists for the tests that
/// need it — every row in every OTHER test in this file defaults
/// `mode` to `'expedited'` (finding 2's index, below), and every row
/// has SOME `flag` value or `NULL`, so creating this unconditionally
/// would make unrelated tests' same-`k` rows collide on an index they
/// aren't exercising.
async fn create_flag_index(pool: &cratestack::sqlx::PgPool) {
    query("CREATE UNIQUE INDEX idx_markers_flag_k ON markers(k) WHERE flag = 'yes'")
        .execute(pool)
        .await
        .expect("create flag partial unique index");
}

/// cratestack#741 finding 2: `mode` carries a literal `@default`, so
/// it is excluded from `CreateMarkerInput`/`insert_values` — the
/// predicate references a column the incoming-row check's synthetic
/// derived table doesn't have. Kept out of `reset_schema` for the same
/// cross-contamination reason as [`create_flag_index`]: `mode` always
/// defaults to `'expedited'`, so this index would make every other
/// test's same-`k` rows collide unrelatedly.
async fn create_mode_index(pool: &cratestack::sqlx::PgPool) {
    query("CREATE UNIQUE INDEX idx_markers_mode_k ON markers(k) WHERE mode = 'expedited'")
        .execute(pool)
        .await
        .expect("create mode partial unique index");
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
    // `mode` is excluded from `CreateMarkerInput` entirely (it carries
    // a literal `@default`, cratestack#741 finding 2) — the database's
    // own column `DEFAULT 'expedited'` fills it.
    cratestack_schema::CreateMarkerInput {
        id,
        k: k.map(str::to_owned),
        status: status.to_owned(),
        payload: payload.to_owned(),
        flag: None,
    }
}

fn marker_with_flag(
    id: i64,
    k: Option<&str>,
    status: &str,
    payload: &str,
    flag: Option<&str>,
) -> cratestack_schema::CreateMarkerInput {
    cratestack_schema::CreateMarkerInput {
        id,
        k: k.map(str::to_owned),
        status: status.to_owned(),
        payload: payload.to_owned(),
        flag: flag.map(str::to_owned),
    }
}

fn active_k_target() -> ConflictTarget {
    ConflictTarget::columns(&["k"]).where_index("status = 'active'")
}

/// cratestack#741 finding 1: `flag` is nullable with no default, so a
/// `flag = 'yes'` predicate hits SQL's three-valued logic (`NULL`, not
/// `false`) whenever the incoming row's `flag` is `NULL`.
fn flag_k_target() -> ConflictTarget {
    ConflictTarget::columns(&["k"]).where_index("flag = 'yes'")
}

/// cratestack#741 finding 2: `mode` carries a literal `@default`, so
/// it is excluded from `insert_values` — the predicate references a
/// column the incoming-row check's synthetic derived table doesn't
/// have unless the fallback is working.
fn mode_k_target() -> ConflictTarget {
    ConflictTarget::columns(&["k"]).where_index("mode = 'expedited'")
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

// ───── #7 cratestack#741 finding 1: NULL predicate result ────────────────
//
// `flag = 'yes'` is SQL's three-valued logic in action: when the
// incoming row's `flag` is `NULL`, the predicate evaluates to `NULL`,
// not `false`. Decoding that as `(bool,)` fails with a
// `sqlx::Error::ColumnDecode` (`UnexpectedNullError`), which
// `cratestack_error_from_sqlx` has no dedicated arm for, so it falls
// through to an opaque `CratestackError::Database` — a valid,
// policy-authorized upsert 500ing whenever the predicate touches a
// NULL column. `NULL` must be treated the same as `false` — the row
// is outside the index's domain — so this must resolve `Inserted`
// twice, not error.

#[tokio::test]
async fn null_predicate_result_is_treated_as_outside_the_domain_not_an_error() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    create_flag_index(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Status is deliberately NOT `"active"` — `idx_markers_active_k`
    // also exists on this table (from `reset_schema`) and this test
    // must isolate the `flag` predicate, not incidentally trip an
    // unrelated index's conflict.
    //
    // `flag` is NULL (never set): `flag = 'yes'` evaluates to NULL for
    // this row, so it must be treated as outside the partial index's
    // domain — like `false`, not an error, and not a conflict.
    let first = cool
        .marker()
        .upsert(marker_with_flag(
            1,
            Some("null-flag-key"),
            "draft",
            "a",
            None,
        ))
        .on_conflict(flag_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("NULL-flag upsert must not error");
    assert!(first.was_inserted(), "got: {first:?}");

    // A second NULL-flag row with the SAME k: still outside the
    // predicate's domain, so it must ALSO insert, not conflict.
    let second = cool
        .marker()
        .upsert(marker_with_flag(
            2,
            Some("null-flag-key"),
            "draft",
            "b",
            None,
        ))
        .on_conflict(flag_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("second NULL-flag upsert must not error");
    assert!(
        second.was_inserted(),
        "NULL flag is outside the index predicate (three-valued logic: NULL, not false, \
         still means 'not in the index'), so no conflict is possible: {second:?}",
    );

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 2, "both NULL-flag rows must exist");
}

// ───── #8 cratestack#741 finding 2: predicate references a @default column ─
//
// `mode` carries a literal `@default("expedited")`, which excludes it
// from `CreateMarkerInput`/`insert_values` (any `@default(...)`, not
// just `auth()`-derived ones, marks a field `is_generated_on_create`).
// A predicate referencing `mode` therefore names a column absent from
// the incoming-row check's synthetic one-row derived table, which
// Postgres rejects with `42703 column "mode" does not exist` —
// unconditionally 500ing every `.do_nothing()` upsert on this conflict
// target before this fix, regardless of what the caller passed.

#[tokio::test]
async fn do_nothing_predicate_referencing_a_default_column_resolves_correctly() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    create_mode_index(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Status is deliberately NOT `"active"` — `idx_markers_active_k`
    // also exists on this table (from `reset_schema`) and this test
    // must isolate the `mode` predicate, not incidentally trip an
    // unrelated index's conflict.
    //
    // `mode` is never in the insert set — the DB's own column DEFAULT
    // ('expedited') fills it every time, so this always falls inside
    // the `mode = 'expedited'` partial index's domain: a genuine
    // conflict must resolve `Existing`, not error.
    let first = cool
        .marker()
        .upsert(marker(1, Some("mode-key"), "pending", "v1"))
        .on_conflict(mode_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("first upsert against a @default-column predicate must not 500");
    assert!(first.was_inserted(), "got: {first:?}");

    let second = cool
        .marker()
        .upsert(marker(
            2,
            Some("mode-key"),
            "pending",
            "v2-should-be-dropped",
        ))
        .on_conflict(mode_k_target())
        .do_nothing()
        .run(&ctx)
        .await
        .expect("second upsert against a @default-column predicate must not 500");
    assert!(
        !second.was_inserted(),
        "mode always defaults to 'expedited', so the same k must genuinely conflict: {second:?}",
    );
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

/// The DO UPDATE path (`.upsert(...).run(...)`, not `.do_nothing()`)
/// does NOT get the same fallback (cratestack#741 finding 2 — see
/// `upsert_exec.rs`'s doc comment for the full reasoning: skipping the
/// pre-probe there would deterministically mislabel every genuine
/// update as a Create, on every call, not just under a race, which is
/// a worse silent defect than a loud error). This test locks in that
/// documented, deliberately-not-fixed limitation as a clear error
/// rather than letting a future change silently start returning wrong
/// data for this schema shape without anyone noticing.
///
/// The error itself is now actionable (cratestack#741 finding 2
/// follow-up, per maintainer request): the raw Postgres `42703` from
/// the incoming-row probe's own derived-table query is narrowly mapped
/// to a `CratestackError::Validation` that names the offending
/// predicate and explains the likely cause/workaround — not the
/// opaque `DatabaseTyped` 500 a caller got before this follow-up. This
/// is the property the test asserts: a real, SPECIFIC, actionable
/// failure — never a wrong Created/Updated classification, and never
/// an unexplained "internal error".
#[tokio::test]
async fn do_update_predicate_referencing_a_default_column_still_errors_clearly() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let err = cool
        .marker()
        .upsert(marker(1, Some("mode-key-do-update"), "active", "v1"))
        .on_conflict(mode_k_target())
        .run(&ctx)
        .await
        .expect_err(
            "known, documented limitation (cratestack#741 finding 2): the DO UPDATE path \
             cannot safely skip its pre-probe, so it still surfaces an error here rather than \
             guessing Created-vs-Updated",
        );

    // A caller-actionable 422 `Validation`, not an opaque 500
    // `DatabaseTyped` — the whole point of the follow-up fix.
    assert_eq!(
        err.code(),
        "VALIDATION_ERROR",
        "expected the narrow 42703-from-the-probe mapping to produce a Validation error, \
         got: {err:?}",
    );
    assert_eq!(err.status_code().as_u16(), 422, "got: {err:?}");

    // The message must name the offending predicate (not just say
    // "something went wrong") and point at the likely cause, so a
    // developer who hits this learns what to fix.
    let message = err.detail().unwrap_or_default();
    assert!(
        message.contains("mode = 'expedited'"),
        "error message must name the offending predicate, got: {message:?}",
    );
    assert!(
        message.contains("@default"),
        "error message must explain the likely cause (a @default(...) column), got: {message:?}",
    );
}

// ───── #9 cratestack#741 finding 2 follow-up: only 42703 falls back ──────
//
// `try_incoming_row_satisfies_predicate`'s savepoint fallback must be
// narrow BY CONSTRUCTION: only Postgres `42703` (undefined column) is
// treated as "unknown, fall back to the authoritative statement" —
// every other probe failure must propagate. This test proves that with
// a predicate that fails for a DIFFERENT reason than a missing column:
// it calls a VOLATILE SQL function that unconditionally raises.
// Postgres's `ON CONFLICT ... WHERE <predicate>` index inference never
// actually INVOKES the predicate — it matches the parsed expression
// tree structurally against `pg_index.indpred`, it does not execute
// it — but the incoming-row probe's `SELECT (<predicate>) FROM (...)`
// genuinely executes it as a real query. So this function can only
// ever fire from the probe, and if it fires, the caller must see ITS
// error, not something else (silence, or a different statement's
// error) from a swallow-then-refail.

async fn create_boom_function(pool: &cratestack::sqlx::PgPool) {
    query(
        "CREATE OR REPLACE FUNCTION cratestack_test_boom() RETURNS boolean AS $$
         BEGIN
             RAISE EXCEPTION 'cratestack_test_boom fired';
         END;
         $$ LANGUAGE plpgsql VOLATILE",
    )
    .execute(pool)
    .await
    .expect("create boom function");
}

#[tokio::test]
async fn non_undefined_column_probe_failure_propagates_not_swallowed() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    create_boom_function(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let bad_target = ConflictTarget::columns(&["k"]).where_index("cratestack_test_boom()");
    let err = cool
        .marker()
        .upsert(marker(1, Some("boom-key"), "active", "v1"))
        .on_conflict(bad_target)
        .do_nothing()
        .run(&ctx)
        .await
        .expect_err("a non-42703 probe failure must propagate, not be silently swallowed");

    // Must NOT be mistaken for the friendly 42703 Validation case —
    // proves the narrow match discriminates by SQLSTATE, not "any
    // probe error".
    assert_ne!(
        err.code(),
        "VALIDATION_ERROR",
        "a RAISE EXCEPTION failure must not be mistaken for the undefined-column case: {err:?}",
    );

    // Must be the probe's OWN failure, not a different, more confusing
    // error from a later statement (e.g. "no unique or exclusion
    // constraint matching the ON CONFLICT specification", which is
    // what would happen if this were silently swallowed and the code
    // fell through to attempt the real INSERT against a predicate no
    // real partial index matches).
    let detail = err.detail().unwrap_or_default();
    assert!(
        detail.contains("cratestack_test_boom fired"),
        "the propagated error must be the probe's own failure: {detail:?}",
    );
    assert!(
        !detail.contains("no unique or exclusion constraint"),
        "must not have fallen through to the real statement: {detail:?}",
    );

    let count: i64 = query("SELECT COUNT(*) FROM markers")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "the failed call must not have inserted anything");
}
