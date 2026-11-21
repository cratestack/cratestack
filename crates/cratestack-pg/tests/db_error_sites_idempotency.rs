//! cratestack#745: every sqlx call site in `SqlxIdempotencyStore` keeps the
//! server's SQLSTATE (`DatabaseTyped`) and reports a lost connection as
//! `Unavailable`. A read-only session makes each write fail with a known
//! SQLSTATE without breaking the table; a closed pool is the lost
//! connection.

mod support;

use std::time::{Duration, SystemTime};

use cratestack::{IdempotencyStore, SqlxIdempotencyStore};
use support::failing_pools::{
    READ_ONLY_SQLSTATE, assert_typed, assert_unavailable, closed_pool, pool_lost_after,
    read_only_pool,
};
use support::pg;

fn in_an_hour() -> SystemTime {
    SystemTime::now() + Duration::from_secs(3600)
}

#[tokio::test]
async fn write_failures_keep_their_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_idempotency")
        .execute(&test_pg.pool)
        .await
        .expect("drop");
    SqlxIdempotencyStore::new(test_pg.pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    let store = SqlxIdempotencyStore::new(read_only_pool(&test_pg.url).await);
    let token = uuid::Uuid::nil();

    assert_typed(
        &store.ensure_schema().await.unwrap_err(),
        READ_ONLY_SQLSTATE,
    );
    assert_typed(
        &store.garbage_collect().await.unwrap_err(),
        READ_ONLY_SQLSTATE,
    );
    assert_typed(
        &store
            .reserve_or_fetch("p", "k", [0; 32], in_an_hour())
            .await
            .unwrap_err(),
        READ_ONLY_SQLSTATE,
    );
    assert_typed(
        &store
            .complete("p", "k", token, 200, b"", b"")
            .await
            .unwrap_err(),
        READ_ONLY_SQLSTATE,
    );
    assert_typed(
        &store.release("p", "k", token).await.unwrap_err(),
        READ_ONLY_SQLSTATE,
    );
}

#[tokio::test]
async fn a_lost_connection_is_unavailable() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let store = SqlxIdempotencyStore::new(closed_pool(&test_pg.url).await);
    let token = uuid::Uuid::nil();

    assert_unavailable(&store.ensure_schema().await.unwrap_err());
    assert_unavailable(&store.garbage_collect().await.unwrap_err());
    assert_unavailable(
        &store
            .reserve_or_fetch("p", "k", [0; 32], in_an_hour())
            .await
            .unwrap_err(),
    );
    assert_unavailable(
        &store
            .complete("p", "k", token, 200, b"", b"")
            .await
            .unwrap_err(),
    );
    assert_unavailable(&store.release("p", "k", token).await.unwrap_err());
}

/// The upsert finds a live reservation and returns no row; the connection is
/// then lost before the read-back of that row, which reports `Unavailable`.
#[tokio::test]
async fn a_connection_lost_at_the_read_back_is_unavailable() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_idempotency")
        .execute(&test_pg.pool)
        .await
        .expect("drop");
    let healthy = SqlxIdempotencyStore::new(test_pg.pool.clone());
    healthy.ensure_schema().await.expect("ensure schema");
    healthy
        .reserve_or_fetch("p", "k", [0; 32], in_an_hour())
        .await
        .expect("first reservation");

    let store = SqlxIdempotencyStore::new(pool_lost_after(&test_pg.url, 1).await);
    let err = store
        .reserve_or_fetch("p", "k", [0; 32], in_an_hour())
        .await
        .unwrap_err();
    assert_unavailable(&err);
}
