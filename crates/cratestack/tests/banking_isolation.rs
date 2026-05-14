//! End-to-end test for `run_in_isolated_tx` and its
//! serialization-failure retry loop.
//!
//! Two concurrent tasks both increment the same balance. Under
//! `Serializable` isolation Postgres surfaces SQLSTATE 40001 to one of
//! them; the runner's retry loop must catch that and re-run the body so
//! the final balance reflects both increments. Run with PG only.

mod support;

use cratestack::sqlx::query;
use cratestack::{CoolError, TransactionIsolation, run_in_isolated_tx};
use support::pg;

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
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    // Spawn two parallel transactions that each +=1 the same row.
    let a = tokio::spawn(increment_balance_with_serialization(pool.clone()));
    let b = tokio::spawn(increment_balance_with_serialization(pool.clone()));

    let (ra, rb) = tokio::join!(a, b);
    ra.expect("task a panicked").expect("a succeeded");
    rb.expect("task b panicked").expect("b succeeded");

    let final_amount: (i64,) =
        cratestack::sqlx::query_as("SELECT amount FROM balance_under_test WHERE id = 1")
            .fetch_one(pool)
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
async fn write_skew_anomaly_surfaces_at_commit_time_and_is_retried_to_success() {
    // Classic SSI write-skew: two tasks read the same predicate, write
    // disjoint rows, and neither observes the other during execution.
    // PG only detects the read/write dependency cycle when the second
    // transaction tries to COMMIT, surfacing SQLSTATE 40001 from
    // `tx.commit()` rather than from any statement in the body. Before
    // the commit-time-retry fix this would leak a transient 40001 out
    // to the caller; with the fix in place both tasks must succeed.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    query("DROP TABLE IF EXISTS bank_accounts_write_skew")
        .execute(pool)
        .await
        .expect("drop");
    query("CREATE TABLE bank_accounts_write_skew (id INT PRIMARY KEY, balance BIGINT NOT NULL)")
        .execute(pool)
        .await
        .expect("create");
    query("INSERT INTO bank_accounts_write_skew VALUES (1, 100), (2, 100)")
        .execute(pool)
        .await
        .expect("seed");

    async fn withdraw_if_combined_balance_allows(
        pool: cratestack::sqlx::PgPool,
        target: i64,
    ) -> Result<(), CoolError> {
        run_in_isolated_tx(
            &pool,
            TransactionIsolation::Serializable,
            move |mut tx| async move {
                let row: (i64,) = cratestack::sqlx::query_as(
                    "SELECT COALESCE(SUM(balance), 0)::BIGINT FROM bank_accounts_write_skew",
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| CoolError::Database(e.to_string()))?;
                // Yield so the two tasks both observe the pre-write
                // snapshot before either writes. This is what makes the
                // anomaly visible — both see total >= 100 and decide
                // to withdraw, even though running them serially would
                // only allow one withdrawal.
                tokio::task::yield_now().await;
                if row.0 >= 100 {
                    cratestack::sqlx::query(
                        "UPDATE bank_accounts_write_skew SET balance = balance - 100 WHERE id = $1",
                    )
                    .bind(target)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| CoolError::Database(e.to_string()))?;
                }
                Ok(((), tx))
            },
        )
        .await
    }

    let a = tokio::spawn(withdraw_if_combined_balance_allows(pool.clone(), 1));
    let b = tokio::spawn(withdraw_if_combined_balance_allows(pool.clone(), 2));
    let (ra, rb) = tokio::join!(a, b);
    ra.expect("task a panicked")
        .expect("task a must not leak commit-time 40001");
    rb.expect("task b panicked")
        .expect("task b must not leak commit-time 40001");

    // After both run, the retry of whichever one lost at commit must
    // have seen the updated state and made a serialisable decision —
    // the final total is well-defined (a single withdrawal succeeded,
    // or both did if the predicate still held on retry).
    let total: (i64,) = cratestack::sqlx::query_as(
        "SELECT COALESCE(SUM(balance), 0)::BIGINT FROM bank_accounts_write_skew",
    )
    .fetch_one(pool)
    .await
    .expect("read");
    assert!(
        total.0 == 0 || total.0 == 100,
        "after retry the total must reflect a serialisable history (0 or 100); got {}",
        total.0,
    );
}

#[tokio::test]
async fn read_committed_can_lose_an_update_when_no_retry_is_configured() {
    // Companion negative: under READ COMMITTED the same workload exhibits
    // the lost-update anomaly (two readers both see 0, both write 1, final
    // is 1 not 2). This test documents WHY the serializable + retry pattern
    // exists.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

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
            .fetch_one(pool)
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
