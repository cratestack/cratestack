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
