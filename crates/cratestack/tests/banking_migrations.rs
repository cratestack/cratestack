//! End-to-end test for the forward-only migration runner.
//!
//! Bare-bones harness — no schema fixture needed. Confirms:
//! - `apply_pending` runs migrations in order and records them in
//!   `cratestack_migrations` with the right checksum;
//! - re-running with the same migrations is idempotent;
//! - mutating an already-applied migration's SQL aborts the whole run
//!   with a checksum-drift error before any new SQL touches the DB.

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::sqlx::query;
use cratestack::{Migration, MigrationStatus};

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static M: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    M.lock().await
}

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let database_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .ok()
}

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
        up: sql.to_owned(),
        down: None,
    }
}

#[tokio::test]
async fn apply_pending_runs_in_order_and_records_each_row() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

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

    let applied = cratestack::apply_pending(&pool, &migrations)
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
            .fetch_all(&pool)
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
    .fetch_one(&pool)
    .await
    .expect("introspect");
    assert_eq!(one.0, 2, "both migration tables should exist");
}

#[tokio::test]
async fn rerunning_apply_pending_is_a_noop() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

    let migrations = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(&pool, &migrations)
        .await
        .expect("first apply");
    let second = cratestack::apply_pending(&pool, &migrations)
        .await
        .expect("second apply must succeed");
    assert!(
        second.is_empty(),
        "second apply should report zero newly-applied migrations, got {second:?}",
    );
}

#[tokio::test]
async fn checksum_drift_aborts_apply_before_running_new_sql() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

    let original = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(&pool, &original)
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
    let result = cratestack::apply_pending(&pool, &drifted).await;
    assert!(result.is_err(), "checksum drift must abort the apply");

    // The new migration must NOT have run — `migration_test_two` is absent.
    let exists: (bool,) = cratestack::sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = 'migration_test_two')",
    )
    .fetch_one(&pool)
    .await
    .expect("introspect");
    assert!(
        !exists.0,
        "follow-on migration must not run when an earlier one has drifted",
    );
}

#[tokio::test]
async fn status_reports_drift_without_changing_state() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

    let original = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id INT PRIMARY KEY);",
    )];
    cratestack::apply_pending(&pool, &original)
        .await
        .expect("apply");

    let drifted = vec![migration(
        "20260101000000_init",
        "CREATE TABLE migration_test_one (id TEXT PRIMARY KEY);",
    )];
    let states = cratestack::status(&pool, &drifted).await.expect("status");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].status, MigrationStatus::ChecksumMismatch);
}

#[tokio::test]
async fn apply_pending_runs_multi_statement_migrations_atomically() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    // Clean up artefacts from this test in addition to the standard
    // reset — `reset` only knows about migration_test_{one,two}.
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations, migration_multi_stmt")
        .execute(&pool)
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

    let applied = cratestack::apply_pending(&pool, &[multi_stmt])
        .await
        .expect("multi-statement migration should apply");
    assert_eq!(applied, vec!["20260201000000_multi_stmt".to_owned()]);

    // Table, index, and seed row must all have landed.
    let table_count: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM information_schema.tables
         WHERE table_name = 'migration_multi_stmt'",
    )
    .fetch_one(&pool)
    .await
    .expect("table check");
    assert_eq!(table_count.0, 1);

    let index_count: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM pg_indexes
         WHERE indexname = 'migration_multi_stmt_label_idx'",
    )
    .fetch_one(&pool)
    .await
    .expect("index check");
    assert_eq!(
        index_count.0, 1,
        "the second statement of the script must run"
    );

    let seed: (i64,) =
        cratestack::sqlx::query_as("SELECT COUNT(*)::BIGINT FROM migration_multi_stmt")
            .fetch_one(&pool)
            .await
            .expect("seed check");
    assert_eq!(seed.0, 1, "the third statement of the script must run");

    // Cleanup so subsequent test runs reset cleanly.
    cratestack::sqlx::query("DROP TABLE migration_multi_stmt")
        .execute(&pool)
        .await
        .expect("teardown");
}

#[tokio::test]
async fn apply_pending_rolls_back_when_a_later_statement_in_a_multi_stmt_fails() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_migrations, migration_partial_apply")
        .execute(&pool)
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

    let result = cratestack::apply_pending(&pool, &[bad]).await;
    assert!(
        result.is_err(),
        "a broken later statement must surface as a migration error",
    );

    let leaked: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM information_schema.tables
         WHERE table_name = 'migration_partial_apply'",
    )
    .fetch_one(&pool)
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
    .fetch_one(&pool)
    .await
    .expect("ledger check");
    assert_eq!(
        recorded.0, 0,
        "a failed multi-statement migration must NOT be recorded as applied",
    );
}
