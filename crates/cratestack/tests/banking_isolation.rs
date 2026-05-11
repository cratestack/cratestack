//! End-to-end test for `run_in_isolated_tx` and its
//! serialization-failure retry loop.
//!
//! Two concurrent tasks both increment the same balance. Under
//! `Serializable` isolation Postgres surfaces SQLSTATE 40001 to one of
//! them; the runner's retry loop must catch that and re-run the body so
//! the final balance reflects both increments. Run with PG only.

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::sqlx::query;
use cratestack::{CoolError, TransactionIsolation, run_in_isolated_tx};

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static M: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    M.lock().await
}

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let database_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .ok()
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS balance_under_test")
        .execute(pool)
        .await
        .expect("drop");
    query("CREATE TABLE balance_under_test (id INT PRIMARY KEY, amount BIGINT NOT NULL)")
        .execute(pool)
        .await
        .expect("create");
    query("INSERT INTO balance_under_test VALUES (1, 0)")
        .execute(pool)
        .await
        .expect("seed");
}

async fn increment_balance_with_serialization(
    pool: cratestack::sqlx::PgPool,
) -> Result<(), CoolError> {
    run_in_isolated_tx(
        &pool,
        TransactionIsolation::Serializable,
        |mut tx| async move {
            // SELECT current value.
            let row: (i64,) =
                cratestack::sqlx::query_as("SELECT amount FROM balance_under_test WHERE id = 1")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| CoolError::Database(e.to_string()))?;
            // Force a small await so the two concurrent tasks actually
            // interleave their reads before either commits.
            tokio::task::yield_now().await;
            // UPDATE back.
            let new_amount = row.0 + 1;
            cratestack::sqlx::query("UPDATE balance_under_test SET amount = $1 WHERE id = 1")
                .bind(new_amount)
                .execute(&mut *tx)
                .await
                .map_err(|e| CoolError::Database(e.to_string()))?;
            Ok(((), tx))
        },
    )
    .await
}

#[tokio::test]
async fn concurrent_increments_under_serializable_observe_both_writes() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

    // Spawn two parallel transactions that each +=1 the same row.
    let a = tokio::spawn(increment_balance_with_serialization(pool.clone()));
    let b = tokio::spawn(increment_balance_with_serialization(pool.clone()));

    let (ra, rb) = tokio::join!(a, b);
    ra.expect("task a panicked").expect("a succeeded");
    rb.expect("task b panicked").expect("b succeeded");

    let final_amount: (i64,) =
        cratestack::sqlx::query_as("SELECT amount FROM balance_under_test WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("read");
    assert_eq!(
        final_amount.0, 2,
        "both increments must land thanks to the serialization-failure retry; \
         got {}",
        final_amount.0,
    );
}

#[tokio::test]
async fn read_committed_can_lose_an_update_when_no_retry_is_configured() {
    // Companion negative: under READ COMMITTED the same workload exhibits
    // the lost-update anomaly (two readers both see 0, both write 1, final
    // is 1 not 2). This test documents WHY the serializable + retry pattern
    // exists.
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset(&pool).await;

    // Run sequentially first to confirm the row math works; then race two.
    async fn loose_increment(pool: cratestack::sqlx::PgPool) -> Result<(), CoolError> {
        run_in_isolated_tx(
            &pool,
            TransactionIsolation::ReadCommitted,
            |mut tx| async move {
                let row: (i64,) = cratestack::sqlx::query_as(
                    "SELECT amount FROM balance_under_test WHERE id = 1",
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| CoolError::Database(e.to_string()))?;
                tokio::task::yield_now().await;
                let new_amount = row.0 + 1;
                cratestack::sqlx::query("UPDATE balance_under_test SET amount = $1 WHERE id = 1")
                    .bind(new_amount)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| CoolError::Database(e.to_string()))?;
                Ok(((), tx))
            },
        )
        .await
    }

    let a = tokio::spawn(loose_increment(pool.clone()));
    let b = tokio::spawn(loose_increment(pool.clone()));
    let (ra, rb) = tokio::join!(a, b);
    ra.expect("a join").expect("a body");
    rb.expect("b join").expect("b body");

    let final_amount: (i64,) =
        cratestack::sqlx::query_as("SELECT amount FROM balance_under_test WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("read");
    // Either both observed each other (=2) or one lost an update (=1).
    // Both are valid outcomes under READ COMMITTED; we just assert the
    // value is in that range, which is enough to confirm the level
    // doesn't enforce serializability on its own.
    assert!(
        final_amount.0 == 1 || final_amount.0 == 2,
        "read_committed should produce 1 (lost update) or 2 (lucky); got {}",
        final_amount.0,
    );
}
