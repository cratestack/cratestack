//! End-to-end tests for the tier-2 builder verbs:
//!
//! * `#3` — `.upsert(input).on_conflict(ConflictTarget::columns(...))`.
//!   Verifies that a composite unique key drives insert-vs-update
//!   branching (so audit/event semantics stay coherent) and that the
//!   `DO UPDATE` only touches the non-key payload.
//! * `#8` — `.find_unique(id).as_detail()` / `.as_list()`. Verifies
//!   that the default policy slot is `detail` (anonymous callers can
//!   reach rows the schema marked detail-readable) and that
//!   `.as_list()` falls back to the list policy slot.
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{ConflictTarget, CratestackContext, Value};
use support::pg;

include_server_schema!(
    "tests/fixtures/builder_extensions_tier2.cstack",
    db = Postgres
);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, articles, pairs")
        .execute(pool)
        .await
        .expect("drop tables");
    query("CREATE TABLE articles (id BIGINT PRIMARY KEY, title TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create articles");
    query(
        "CREATE TABLE pairs (
            id BIGINT PRIMARY KEY,
            scope TEXT NOT NULL,
            key TEXT NOT NULL,
            payload TEXT NOT NULL,
            UNIQUE(scope, key)
        )",
    )
    .execute(pool)
    .await
    .expect("create pairs");
}

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("tier2-001")
}

// ───── #8 as_detail() / as_list() ────────────────────────────────────────────

#[tokio::test]
async fn find_unique_default_routes_through_detail_policy() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    // Seed an article directly so we don't depend on create policy.
    query("INSERT INTO articles (id, title) VALUES (1, 'public')")
        .execute(pool)
        .await
        .unwrap();

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // Schema says: @@allow("detail", auth() == null) — anonymous detail
    // lookups are allowed. The bug fix routes find_unique through the
    // detail slot by default so this returns Some(article).
    let anon = CratestackContext::anonymous();
    let found = cool
        .article()
        .bind(anon)
        .find_unique(1)
        .run()
        .await
        .expect("find_unique with detail policy succeeds for anon");
    assert!(found.is_some(), "anonymous detail lookup should resolve");
}

#[tokio::test]
async fn find_unique_as_list_falls_back_to_list_policy() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO articles (id, title) VALUES (2, 'gated')")
        .execute(pool)
        .await
        .unwrap();

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // List policy is `auth() != null` — anonymous as_list() lookup
    // must return None (no row matches the policy clause).
    let anon = CratestackContext::anonymous();
    let denied = cool
        .article()
        .bind(anon)
        .find_unique(2)
        .as_list()
        .run()
        .await
        .expect("query itself does not error");
    assert!(denied.is_none(), "as_list() must apply list policy");

    // The same row IS visible under the detail policy.
    let anon2 = CratestackContext::anonymous();
    let allowed = cool
        .article()
        .bind(anon2)
        .find_unique(2)
        .as_detail()
        .run()
        .await
        .expect("as_detail() succeeds for anon");
    assert!(allowed.is_some());
}

// ───── #3 composite-key upsert ───────────────────────────────────────────────

#[tokio::test]
async fn composite_upsert_inserts_then_updates_on_natural_key() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    let target = ConflictTarget::columns(&["scope", "key"]);

    let first = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 100,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .on_conflict(target)
        .run(&ctx)
        .await
        .expect("first composite upsert inserts");
    assert_eq!(first.payload, "v1");

    // Second upsert with a DIFFERENT id but the same (scope, key) must
    // update the existing row — the conflict target is the natural key,
    // not the PK. (Caller-supplied id 999 here is effectively ignored on
    // the update branch because @id is excluded from
    // `upsert_update_columns`.)
    let second = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 999,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v2".into(),
        })
        .on_conflict(target)
        .run(&ctx)
        .await
        .expect("second composite upsert updates");
    assert_eq!(second.payload, "v2");
    assert_eq!(second.id, first.id, "natural-key match leaves PK alone");

    // Only one row exists in the table.
    let count: i64 = query("SELECT COUNT(*) FROM pairs")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1);

    // Audit must show both operations against the same conflict row.
    let audit: Vec<String> =
        query("SELECT operation FROM cratestack_audit WHERE model = 'Pair' ORDER BY occurred_at")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.get::<String, _>("operation"))
            .collect();
    assert_eq!(audit, vec!["create".to_string(), "update".to_string()]);
}

#[tokio::test]
async fn composite_upsert_with_missing_conflict_column_rejects_with_validation_error() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    // The conflict references a column that does NOT exist in the input
    // — the input only carries id/scope/key/payload, not "nonexistent".
    // The runtime catches this before any SQL runs.
    let err = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "s".into(),
            key: "k".into(),
            payload: "p".into(),
        })
        .on_conflict(ConflictTarget::columns(&["scope", "nonexistent"]))
        .run(&ctx)
        .await
        .expect_err("missing conflict column must fail");
    let detail = err.detail().unwrap_or_default();
    assert!(
        detail.contains("nonexistent"),
        "error should name the missing column, got: {detail:?}",
    );
}

#[tokio::test]
async fn pk_default_upsert_unchanged_after_composite_refactor() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // No `.on_conflict()` call — must behave identically to pre-refactor
    // upsert (conflict on `id`).
    let first = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 7,
            scope: "x".into(),
            key: "y".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("default upsert inserts");
    assert_eq!(first.id, 7);

    let second = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 7,
            scope: "x".into(),
            key: "y".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect("default upsert updates on PK");
    assert_eq!(second.payload, "v2");
}

// ───── #267 regression: SQLSTATE/constraint must survive create/update ──────
//
// Every generated write query used to collapse `sqlx::Error` into
// `CratestackError::Database(error.to_string())`, discarding the driver's
// SQLSTATE and constraint name before the error reached application
// code. These hit `pairs`' real `UNIQUE(scope, key)` constraint through
// the ordinary `.create()` / `.update()` delegates (not a hand-built
// `CratestackError`), so they only pass once `cratestack_error_from_sqlx` is
// actually wired into `create_exec.rs` / `update_exec.rs`.

#[tokio::test]
async fn create_with_unique_constraint_violation_preserves_sqlstate_and_constraint() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("first create succeeds");

    // Same (scope, key), different id — violates `UNIQUE(scope, key)`.
    let err = cool
        .pair()
        .create(cratestack_schema::CreatePairInput {
            id: 2,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect_err("duplicate (scope, key) must be rejected by the database");

    assert_eq!(
        err.db_sqlstate(),
        Some("23505"),
        "unique violation must surface its real SQLSTATE, got: {err}",
    );
    assert!(
        err.db_constraint().is_some(),
        "unique violation must surface the constraint name, got: {err}",
    );
}

#[tokio::test]
async fn update_with_unique_constraint_violation_preserves_sqlstate_and_constraint() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("first pair created");
    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 2,
            scope: "session".into(),
            key: "xyz".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect("second pair created");

    // Retarget pair 2's key onto pair 1's (scope, key).
    let err = cool
        .pair()
        .update(2_i64)
        .set(cratestack_schema::UpdatePairInput {
            scope: None,
            key: Some("abc".into()),
            payload: None,
        })
        .run(&ctx)
        .await
        .expect_err("update onto an existing (scope, key) must be rejected by the database");

    assert_eq!(
        err.db_sqlstate(),
        Some("23505"),
        "unique violation on update must surface its real SQLSTATE, got: {err}",
    );
    assert!(
        err.db_constraint().is_some(),
        "unique violation on update must surface the constraint name, got: {err}",
    );
}

#[tokio::test]
async fn single_row_create_returns_conflict_on_unique_violation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("first create succeeds");

    // Same (scope, key), different id — violates `UNIQUE(scope, key)`.
    let err = cool
        .pair()
        .create(cratestack_schema::CreatePairInput {
            id: 2,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect_err("duplicate (scope, key) must be rejected as a conflict");

    assert_eq!(
        err.code(),
        "CONFLICT",
        "single-row create on unique violation must return CONFLICT (409), got: {err}",
    );
}

#[tokio::test]
async fn single_row_update_returns_conflict_on_unique_violation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("first pair created");
    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 2,
            scope: "session".into(),
            key: "xyz".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect("second pair created");

    // Retarget pair 2's key onto pair 1's (scope, key).
    let err = cool
        .pair()
        .update(2_i64)
        .set(cratestack_schema::UpdatePairInput {
            scope: None,
            key: Some("abc".into()),
            payload: None,
        })
        .run(&ctx)
        .await
        .expect_err("update onto an existing (scope, key) must be rejected as a conflict");

    assert_eq!(
        err.code(),
        "CONFLICT",
        "single-row update on unique violation must return CONFLICT (409), got: {err}",
    );
}

#[tokio::test]
async fn single_row_upsert_returns_conflict_on_non_conflict_target_unique_violation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Insert the first pair with id=1 — this is the (scope, key) the
    // insert-branch upsert below will collide with.
    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 1,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v1".into(),
        })
        .run(&ctx)
        .await
        .expect("first pair created");
    // An unrelated second row so the schema has more than one (scope,
    // key) pair in play before the conflicting upsert below.
    cool.pair()
        .create(cratestack_schema::CreatePairInput {
            id: 2,
            scope: "data".into(),
            key: "xyz".into(),
            payload: "v2".into(),
        })
        .run(&ctx)
        .await
        .expect("second pair created");

    // Upsert id=3 (new, will insert) with (scope, key) = ("session", "abc")
    // which already exists on id=1. This should fail on the unique
    // constraint (the insert branch, not the PK-conflict/update branch).
    let err = cool
        .pair()
        .upsert(cratestack_schema::CreatePairInput {
            id: 3,
            scope: "session".into(),
            key: "abc".into(),
            payload: "v3".into(),
        })
        .run(&ctx)
        .await
        .expect_err("upsert insert branch on unique violation must be rejected as a conflict");

    assert_eq!(
        err.code(),
        "CONFLICT",
        "single-row upsert on unique violation must return CONFLICT (409), got: {err}",
    );
}
