//! End-to-end test for the forward-only migration runner.
//!
//! Bare-bones harness — no schema fixture needed. Confirms:
//! - `apply_pending` runs migrations in order and records them in
//!   `cratestack_migrations` with the right checksum;
//! - re-running with the same migrations is idempotent;
//! - mutating an already-applied migration's SQL aborts the whole run
//!   with a checksum-drift error before any new SQL touches the DB.

mod support;

use cratestack::sqlx::query;
use cratestack::{Migration, MigrationStatus};
use support::pg;

async fn reset(pool: &cratestack::sqlx::PgPool) {
    // Clean both the runner's own tracking table and the test artefacts.
    query("DROP TABLE IF EXISTS cratestack_migrations, migration_test_one, migration_test_two")
        .execute(pool)
        .await
        .expect("drop");
}

fn migration(id: &str, sql: &str) -> Migration {
    Migration {
        id: id.to_owned(),
        description: format!("test migration {id}"),
        up_pre: None,
        up: sql.to_owned(),
        down: None,
    }
}

#[tokio::test]
async fn apply_pending_runs_in_order_and_records_each_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let migrations = vec![
        migration(
            "20260101000000_one",
            "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
        ),
        migration(
            "20260102000000_two",
            "CREATE TABLE migration_test_two (id INT PRIMARY KEY);",
        ),
    ];

    let applied = cratestack::apply_pending(pool, &migrations)
        .await
        .expect("apply");
    assert_eq!(
        applied,
        vec![
            "20260101000000_one".to_owned(),
            "20260102000000_two".to_owned(),
        ],
        "migrations must be reported in apply order",
    );

    let rows: Vec<(String,)> =
        cratestack::sqlx::query_as("SELECT id FROM cratestack_migrations ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("read migrations");
    assert_eq!(
        rows.iter().map(|(id,)| id.as_str()).collect::<Vec<_>>(),
        vec!["20260101000000_one", "20260102000000_two"],
    );

    // Both real tables exist.
    let one: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM information_schema.tables \
         WHERE table_name IN ('migration_test_one', 'migration_test_two')",
    )
    .fetch_one(pool)
    .await
    .expect("introspect");
    assert_eq!(one.0, 2, "both migration tables should exist");
}

#[tokio::test]
async fn rerunning_apply_pending_is_a_noop() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let migrations = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(pool, &migrations)
        .await
        .expect("first apply");
    let second = cratestack::apply_pending(pool, &migrations)
        .await
        .expect("second apply must succeed");
    assert!(
        second.is_empty(),
        "second apply should report zero newly-applied migrations, got {second:?}",
    );
}

#[tokio::test]
async fn checksum_drift_aborts_apply_before_running_new_sql() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let original = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(pool, &original)
        .await
        .expect("first apply");

    // Mutate the already-applied migration's SQL — banks treat this as a
    // release-process failure to be resolved by humans, never silently
    // overwritten.
    let drifted = vec![
        migration(
            "20260101000000_init",
            "CREATE TABLE migration_test_one (id BIGINT PRIMARY KEY);",
        ),
        migration(
            "20260102000000_two",
            "CREATE TABLE migration_test_two (id INT PRIMARY KEY);",
        ),
    ];
    let result = cratestack::apply_pending(pool, &drifted).await;
    assert!(result.is_err(), "checksum drift must abort the apply");

    // The new migration must NOT have run — `migration_test_two` is absent.
    let exists: (bool,) = cratestack::sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = 'migration_test_two')",
    )
    .fetch_one(pool)
    .await
    .expect("introspect");
    assert!(
        !exists.0,
        "follow-on migration must not run when an earlier one has drifted",
    );
}

#[tokio::test]
async fn status_reports_drift_without_changing_state() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let original = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(pool, &original)
        .await
        .expect("apply");

    let drifted = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id TEXT PRIMARY KEY);",
    )];
    let states = cratestack::status(pool, &drifted).await.expect("status");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].status, MigrationStatus::ChecksumMismatch);
}

#[tokio::test]
async fn apply_pending_runs_multi_statement_migrations_atomically() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    // Clean up artefacts from this test in addition to the standard
    // reset — `reset` only knows about migration_test_{one,two}.
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations, migration_multi_stmt")
        .execute(pool)
        .await
        .expect("drop");

    // Pre-fix this entire `.up` was sent as a single `sqlx::query` call,
    // which Postgres rejects (prepared statements only accept one
    // command). Banks routinely ship multi-statement migrations like
    // `CREATE TABLE …; CREATE INDEX …; INSERT INTO seed …;` — split
    // execution inside the migration's own transaction lets the whole
    // bundle land atomically.
    let multi_stmt = migration(
        "20260201000000_multi_stmt",
        "CREATE TABLE migration_multi_stmt (id INT PRIMARY KEY, label TEXT NOT NULL);\n\
         CREATE INDEX migration_multi_stmt_label_idx ON migration_multi_stmt (label);\n\
         INSERT INTO migration_multi_stmt (id, label) VALUES (1, 'seed');",
    );

    let applied = cratestack::apply_pending(pool, &[multi_stmt])
        .await
        .expect("multi-statement migration should apply");
    assert_eq!(applied, vec!["20260201000000_multi_stmt".to_owned()]);

    // Table, index, and seed row must all have landed.
    let table_count: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM information_schema.tables
         WHERE table_name = 'migration_multi_stmt'",
    )
    .fetch_one(pool)
    .await
    .expect("table check");
    assert_eq!(table_count.0, 1);

    let index_count: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM pg_indexes
         WHERE indexname = 'migration_multi_stmt_label_idx'",
    )
    .fetch_one(pool)
    .await
    .expect("index check");
    assert_eq!(
        index_count.0, 1,
        "the second statement of the script must run"
    );

    let seed: (i64,) =
        cratestack::sqlx::query_as("SELECT COUNT(*)::BIGINT FROM migration_multi_stmt")
            .fetch_one(pool)
            .await
            .expect("seed check");
    assert_eq!(seed.0, 1, "the third statement of the script must run");

    // Cleanup so subsequent test runs reset cleanly.
    cratestack::sqlx::query("DROP TABLE migration_multi_stmt")
        .execute(pool)
        .await
        .expect("teardown");
}

/// Regression coverage for issue #270: `apply_pending` used to split a
/// migration's `up` SQL on every literal `;`, which cut straight through
/// dollar-quoted PL/pgSQL bodies — any `CREATE FUNCTION ... AS $$ ... $$`
/// containing an internal `;` failed with "unterminated dollar-quoted
/// string". The fix sends the whole `up` script as one batch via
/// `sqlx::raw_sql` instead of splitting client-side, letting Postgres
/// itself handle the dollar-quoting.
#[tokio::test]
async fn apply_pending_survives_dollar_quoted_plpgsql_function_body() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations")
        .execute(pool)
        .await
        .expect("drop");
    cratestack::sqlx::query("DROP FUNCTION IF EXISTS migration_dollar_quoted_touch() CASCADE")
        .execute(pool)
        .await
        .expect("drop function");

    // Exact shape from the issue repro: a trigger function body whose
    // `$$...$$` block contains an internal `;`.
    let dollar_quoted = migration(
        "20260203000000_dollar_quoted_fn",
        "CREATE FUNCTION migration_dollar_quoted_touch() RETURNS trigger AS $$\n\
         BEGIN\n\
         \x20   NEW.updated_at = now();\n\
         \x20   RETURN NEW;\n\
         END;\n\
         $$ LANGUAGE plpgsql;",
    );

    let applied = cratestack::apply_pending(pool, &[dollar_quoted])
        .await
        .expect("a dollar-quoted function body should apply intact");
    assert_eq!(applied, vec!["20260203000000_dollar_quoted_fn".to_owned()]);

    let exists: (bool,) = cratestack::sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM pg_proc WHERE proname = 'migration_dollar_quoted_touch')",
    )
    .fetch_one(pool)
    .await
    .expect("introspect");
    assert!(
        exists.0,
        "the function should have been created verbatim, not truncated at the first internal `;`",
    );

    // Cleanup so subsequent runs reset cleanly.
    cratestack::sqlx::query("DROP FUNCTION migration_dollar_quoted_touch() CASCADE")
        .execute(pool)
        .await
        .expect("teardown");
}

/// Companion to the repro above: proves a dollar-quoted trigger function
/// applied through `apply_pending` is not just accepted as DDL but is a
/// working trigger — the motivating case from the issue (an `updated_at`
/// touch trigger, since Postgres has no `ON UPDATE` column default).
#[tokio::test]
async fn apply_pending_dollar_quoted_trigger_fires_on_update() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations, migration_trigger_target")
        .execute(pool)
        .await
        .expect("drop");
    cratestack::sqlx::query("DROP FUNCTION IF EXISTS migration_trigger_touch() CASCADE")
        .execute(pool)
        .await
        .expect("drop function");

    let with_trigger = migration(
        "20260204000000_trigger",
        "CREATE TABLE migration_trigger_target (\n\
         \x20   id INT PRIMARY KEY,\n\
         \x20   updated_at TIMESTAMPTZ NOT NULL DEFAULT now()\n\
         );\n\
         CREATE FUNCTION migration_trigger_touch() RETURNS trigger AS $$\n\
         BEGIN\n\
         \x20   NEW.updated_at = now();\n\
         \x20   RETURN NEW;\n\
         END;\n\
         $$ LANGUAGE plpgsql;\n\
         CREATE TRIGGER migration_trigger_target_touch\n\
         \x20   BEFORE UPDATE ON migration_trigger_target\n\
         \x20   FOR EACH ROW\n\
         \x20   EXECUTE FUNCTION migration_trigger_touch();",
    );

    cratestack::apply_pending(pool, &[with_trigger])
        .await
        .expect("table + dollar-quoted function + trigger should apply as one script");

    query(
        "INSERT INTO migration_trigger_target (id, updated_at) VALUES (1, '2020-01-01T00:00:00Z')",
    )
    .execute(pool)
    .await
    .expect("seed row");
    let (before,): (chrono::DateTime<chrono::Utc>,) =
        cratestack::sqlx::query_as("SELECT updated_at FROM migration_trigger_target WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("read before");

    query("UPDATE migration_trigger_target SET id = 1 WHERE id = 1")
        .execute(pool)
        .await
        .expect("trigger update");
    let (after,): (chrono::DateTime<chrono::Utc>,) =
        cratestack::sqlx::query_as("SELECT updated_at FROM migration_trigger_target WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("read after");

    assert!(
        after > before,
        "the trigger created by the migration should have bumped updated_at on UPDATE",
    );

    // Cleanup so subsequent runs reset cleanly.
    cratestack::sqlx::query("DROP TABLE migration_trigger_target CASCADE")
        .execute(pool)
        .await
        .expect("teardown table");
    cratestack::sqlx::query("DROP FUNCTION migration_trigger_touch() CASCADE")
        .execute(pool)
        .await
        .expect("teardown function");
}

#[tokio::test]
async fn apply_pending_rolls_back_when_a_later_statement_in_a_multi_stmt_fails() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations, migration_partial_apply")
        .execute(pool)
        .await
        .expect("drop");

    // The second statement is intentionally invalid (references a column
    // that doesn't exist). If we executed statements outside a tx, the
    // CREATE TABLE in the first statement would leak. Inside the tx,
    // the failure must roll the entire migration back and the
    // `cratestack_migrations` row must NOT be recorded.
    let bad = migration(
        "20260202000000_partial",
        "CREATE TABLE migration_partial_apply (id INT PRIMARY KEY);\n\
         CREATE INDEX bad_idx ON migration_partial_apply (column_that_does_not_exist);",
    );

    let result = cratestack::apply_pending(pool, &[bad]).await;
    assert!(
        result.is_err(),
        "a broken later statement must surface as a migration error",
    );

    let leaked: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM information_schema.tables
         WHERE table_name = 'migration_partial_apply'",
    )
    .fetch_one(pool)
    .await
    .expect("table check");
    assert_eq!(
        leaked.0, 0,
        "the first statement must roll back when the second fails",
    );

    let recorded: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM cratestack_migrations
         WHERE id = '20260202000000_partial'",
    )
    .fetch_one(pool)
    .await
    .expect("ledger check");
    assert_eq!(
        recorded.0, 0,
        "a failed multi-statement migration must NOT be recorded as applied",
    );
}

/// The end-to-end claim of cratestack#843: an operator's `up.pre.sql`
/// actually runs, and runs *before* `up.sql`.
///
/// Deliberately shaped as the issue's own reproduction — promote a
/// nullable column to NOT NULL against a table that already has a NULL
/// row — because the whole defect was that this passed on an empty
/// table and failed on a real one. The precondition assert is the test:
/// without the pre-script the migration must fail, or the success below
/// proves nothing.
#[tokio::test]
async fn up_pre_sql_runs_before_up_sql_in_the_same_transaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    async fn seed_table_with_a_null_row(pool: &cratestack::sqlx::PgPool) {
        query("DROP TABLE IF EXISTS cratestack_migrations, migration_up_pre")
            .execute(pool)
            .await
            .expect("drop");
        query("CREATE TABLE migration_up_pre (id INT PRIMARY KEY, version BIGINT)")
            .execute(pool)
            .await
            .expect("create");
        query("INSERT INTO migration_up_pre (id, version) VALUES (1, NULL)")
            .execute(pool)
            .await
            .expect("seed");
    }

    let promote = "ALTER TABLE migration_up_pre ALTER COLUMN version SET NOT NULL;";
    let backfill = "UPDATE migration_up_pre SET version = 0 WHERE version IS NULL;";

    // Precondition: the migration genuinely blocks without the
    // pre-script. If this ever starts passing, the case below is
    // vacuous and this test is guarding nothing.
    seed_table_with_a_null_row(pool).await;
    let without_pre = migration("20260201000000_promote", promote);
    let error = cratestack::apply_pending(pool, &[without_pre])
        .await
        .expect_err("NOT NULL promotion must fail while a NULL row exists");
    assert!(
        error.to_string().contains("contains null values"),
        "expected a NOT NULL violation, got: {error}"
    );

    // With the pre-script, the same migration succeeds — which is only
    // possible if `up_pre` ran, and ran first.
    seed_table_with_a_null_row(pool).await;
    let mut with_pre = migration("20260201000000_promote", promote);
    with_pre.up_pre = Some(backfill.to_owned());
    let applied = cratestack::apply_pending(pool, &[with_pre])
        .await
        .expect("backfill should unblock the promotion");
    assert_eq!(applied, vec!["20260201000000_promote".to_owned()]);

    let (version,): (i64,) = cratestack::sqlx::query_as("SELECT version FROM migration_up_pre")
        .fetch_one(pool)
        .await
        .expect("row survives");
    assert_eq!(version, 0, "the backfill's value should be what landed");
}

/// A failure in `up.sql` must roll back `up.pre.sql` too — they are one
/// transaction, so a half-applied backfill must not survive.
#[tokio::test]
async fn a_failing_up_sql_rolls_back_the_pre_script() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    query("DROP TABLE IF EXISTS cratestack_migrations, migration_up_pre_rollback")
        .execute(pool)
        .await
        .expect("drop");
    query("CREATE TABLE migration_up_pre_rollback (id INT PRIMARY KEY, note TEXT)")
        .execute(pool)
        .await
        .expect("create");
    query("INSERT INTO migration_up_pre_rollback (id, note) VALUES (1, 'original')")
        .execute(pool)
        .await
        .expect("seed");

    let mut doomed = migration(
        "20260202000000_doomed",
        "ALTER TABLE migration_up_pre_rollback ADD COLUMN note TEXT;",
    );
    doomed.up_pre =
        Some("UPDATE migration_up_pre_rollback SET note = 'rewritten by pre-script';".to_owned());

    cratestack::apply_pending(pool, &[doomed])
        .await
        .expect_err("adding a duplicate column must fail");

    let (note,): (String,) =
        cratestack::sqlx::query_as("SELECT note FROM migration_up_pre_rollback")
            .fetch_one(pool)
            .await
            .expect("row survives");
    assert_eq!(
        note, "original",
        "the pre-script's write must have rolled back with the failed up.sql"
    );
}

/// Editing a pre-script after it has been applied is drift, exactly as
/// editing `up.sql` is. Before `up_pre` existed, a hand-written
/// pre-script was invisible to this check.
#[tokio::test]
async fn editing_an_applied_up_pre_sql_is_detected_as_drift() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    query("DROP TABLE IF EXISTS cratestack_migrations, migration_up_pre_drift")
        .execute(pool)
        .await
        .expect("drop");

    let mut applied = migration(
        "20260203000000_drift",
        "CREATE TABLE migration_up_pre_drift (id INT PRIMARY KEY);",
    );
    applied.up_pre = Some("SELECT 1;".to_owned());
    cratestack::apply_pending(pool, std::slice::from_ref(&applied))
        .await
        .expect("first apply");

    let mut edited = applied.clone();
    edited.up_pre = Some("SELECT 2;".to_owned());
    let error = cratestack::apply_pending(pool, &[edited])
        .await
        .expect_err("an edited pre-script must be reported as drift");
    assert!(
        error.to_string().contains("its SQL has changed"),
        "expected a drift error, got: {error}"
    );
}
