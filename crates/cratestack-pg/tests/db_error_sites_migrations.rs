//! cratestack#745: every sqlx call site in the migration runner keeps the
//! server's SQLSTATE, constraint and table (`DatabaseTyped`) and reports a
//! lost connection as `Unavailable`, instead of flattening both into
//! `Database(error.to_string())`. Each test fails at exactly one site, so
//! reverting that site to the stringified form fails that test.

mod support;

use cratestack::sqlx::query;
use cratestack::{Migration, apply_pending, ensure_migrations_table, status};
use support::failing_pools::{
    READ_ONLY_SQLSTATE, assert_typed, assert_unavailable, closed_pool, pool_lost_after,
    read_only_pool,
};
use support::pg;

fn migration(id: &str, description: &str, up_pre: Option<&str>, up: &str) -> Migration {
    Migration {
        id: id.to_owned(),
        description: description.to_owned(),
        up_pre: up_pre.map(str::to_owned),
        up: up.to_owned(),
        down: None,
    }
}

/// A clean `cratestack_migrations` plus, optionally, one extra statement
/// that makes a later step of the runner fail.
async fn reset(pool: &cratestack::sqlx::PgPool, extra: &str) {
    cratestack::sqlx::raw_sql(cratestack::sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS cratestack_migrations, sites_745 CASCADE;
         DROP FUNCTION IF EXISTS sites_745_trip();
         {}
         {extra}",
        cratestack::MIGRATIONS_TABLE_DDL
    )))
    .execute(pool)
    .await
    .expect("reset");
}

/// `up.pre.sql` (site: the `up_pre` execute) and `up` (site: the `up`
/// execute) each surface the SQLSTATE, constraint and table of what
/// failed in them.
#[tokio::test]
async fn up_pre_and_up_failures_keep_their_sqlstate_constraint_and_table() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool, "").await;

    let violates_check =
        "CREATE TABLE sites_745 (qty INT CONSTRAINT sites_745_qty CHECK (qty >= 0));
                          INSERT INTO sites_745 VALUES (-1);";
    let raises_custom = "DO $$ BEGIN RAISE EXCEPTION 'refused' USING ERRCODE = 'A2745'; END $$;";

    let in_up_pre = migration("1_up_pre", "d", Some(violates_check), "SELECT 1");
    let err = apply_pending(pool, &[in_up_pre]).await.unwrap_err();
    assert_typed(&err, "23514");
    assert_eq!(err.db_constraint(), Some("sites_745_qty"), "{err:?}");
    assert_eq!(err.db_table(), Some("sites_745"), "{err:?}");

    let in_up = migration("2_up", "d", None, raises_custom);
    let err = apply_pending(pool, &[in_up]).await.unwrap_err();
    assert_typed(&err, "A2745");

    let rows: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM cratestack_migrations")
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(rows, 0, "a failed migration records nothing");
}

/// The `INSERT INTO cratestack_migrations` step and the `COMMIT` that can
/// fail on a deferred constraint each keep the SQLSTATE.
#[tokio::test]
async fn record_and_commit_failures_keep_their_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(
        pool,
        "ALTER TABLE cratestack_migrations
             ADD CONSTRAINT sites_745_no_forbidden CHECK (description <> 'forbidden');
         CREATE FUNCTION sites_745_trip() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN RAISE EXCEPTION 'deferred refusal' USING ERRCODE = 'A2746'; END $$;
         CREATE CONSTRAINT TRIGGER sites_745_commit_trip
             AFTER INSERT ON cratestack_migrations
             DEFERRABLE INITIALLY DEFERRED
             FOR EACH ROW WHEN (NEW.description = 'trip-at-commit')
             EXECUTE FUNCTION sites_745_trip();",
    )
    .await;

    // Fails at the INSERT: the CHECK on the tracking table.
    let at_insert = migration("1_insert", "forbidden", None, "SELECT 1");
    let err = apply_pending(pool, &[at_insert]).await.unwrap_err();
    assert_typed(&err, "23514");
    assert_eq!(
        err.db_constraint(),
        Some("sites_745_no_forbidden"),
        "{err:?}"
    );
    assert_eq!(err.db_table(), Some("cratestack_migrations"), "{err:?}");

    // Fails at COMMIT: the deferred trigger only runs there.
    let at_commit = migration("2_commit", "trip-at-commit", None, "SELECT 1");
    let err = apply_pending(pool, &[at_commit]).await.unwrap_err();
    assert_typed(&err, "A2746");
}

/// The `status` read keeps the SQLSTATE of a tracking table that has
/// the wrong shape.
#[tokio::test]
async fn status_failure_keeps_its_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(
        pool,
        "ALTER TABLE cratestack_migrations RENAME COLUMN checksum TO checksum_old;",
    )
    .await;

    let err = status(pool, &[]).await.unwrap_err();
    assert_typed(&err, "42703");
    let err = apply_pending(pool, &[]).await.unwrap_err();
    assert_typed(&err, "42703");
}

/// The tracking-table DDL keeps the SQLSTATE when the session cannot write.
#[tokio::test]
async fn ensure_migrations_table_failure_keeps_its_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let read_only = read_only_pool(&test_pg.url).await;

    let err = ensure_migrations_table(&read_only).await.unwrap_err();
    assert_typed(&err, READ_ONLY_SQLSTATE);
}

/// A lost connection is `Unavailable` (503) at every step that can reach it,
/// not a 500.
#[tokio::test]
async fn a_lost_connection_is_unavailable() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let closed = closed_pool(&test_pg.url).await;

    assert_unavailable(&ensure_migrations_table(&closed).await.unwrap_err());
    assert_unavailable(&status(&closed, &[]).await.unwrap_err());
    assert_unavailable(&apply_pending(&closed, &[]).await.unwrap_err());

    // The shared pool is untouched.
    query("SELECT 1")
        .execute(&test_pg.pool)
        .await
        .expect("shared pool still open");
}

/// The connection is lost after the `status` read and before the
/// per-migration transaction opens: `begin` reports `Unavailable`.
#[tokio::test]
async fn a_connection_lost_at_begin_is_unavailable() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool, "").await;
    let pool = pool_lost_after(&test_pg.url, 2).await;

    let pending = migration("1_pending", "d", None, "SELECT 1");
    let err = apply_pending(&pool, &[pending]).await.unwrap_err();
    assert_unavailable(&err);
}
