//! Where policy reads run when a write executes inside a transaction the
//! caller opened *without* `@isolation`: `db.transaction(..)`, `run_in_tx`,
//! `run_in_isolated_tx` and `batch_create`. They run on that transaction, the
//! same as an `@isolation` attempt's (`procedure_isolation_policy.rs`,
//! docs/design/procedure-isolation.md §4.1, cratestack#1117): no second pooled
//! connection, the caller's own uncommitted writes are visible to them, and
//! under `REPEATABLE READ` / `SERIALIZABLE` they read the caller's snapshot.
//!
//! Until 0.14.2 these reads went to the pool, which is what each case below
//! used to pin; the expectations here are the flipped ones.

use std::time::{Duration, Instant};

use futures_util::future::join_all;

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    CratestackContext, CratestackError, TransactionIsolation, Value, run_in_isolated_tx,
};

include_server_schema!("tests/fixtures/policy_caller_tx.cstack", db = Postgres);

mod support;

use cratestack_schema::{
    CreateCallerTxAuditedInput, CreateCallerTxFolderInput, CreateCallerTxItemInput,
};
use cratestack_schema::{
    CreateCallerTxOwnerInput, UpdateCallerTxDocInput, UpdateCallerTxOwnerInput,
};
use support::pg;

type Pool = cratestack::sqlx::PgPool;

/// Longer than any of these writes takes on a connection of its own, shorter
/// than the pool's `acquire_timeout`, so a wait for a second connection fails.
const BUDGET: Duration = Duration::from_secs(1);

async fn one_connection_pool(url: &str) -> Pool {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect(url)
        .await
        .expect("one-connection pool")
}

fn caller() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn sql(pool: &Pool, statement: &str) {
    cratestack::sqlx::query(cratestack::sqlx::AssertSqlSafe(statement.to_owned()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

async fn reset(pool: &Pool) {
    sql(
        pool,
        "DROP TABLE IF EXISTS caller_tx_owners, caller_tx_items, caller_tx_folders, \
         caller_tx_docs, caller_tx_auditeds",
    )
    .await;
    sql(
        pool,
        "CREATE TABLE caller_tx_owners (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL)",
    )
    .await;
    sql(
        pool,
        "CREATE TABLE caller_tx_items (id BIGINT PRIMARY KEY, owner_row_id BIGINT NOT NULL)",
    )
    .await;
    sql(
        pool,
        "CREATE TABLE caller_tx_auditeds (id BIGINT PRIMARY KEY, owner_row_id BIGINT NOT NULL)",
    )
    .await;
    sql(
        pool,
        "CREATE TABLE caller_tx_folders (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
         parent_id BIGINT NOT NULL)",
    )
    .await;
}

async fn reset_docs(pool: &Pool) {
    sql(
        pool,
        "CREATE TABLE caller_tx_docs (id BIGINT PRIMARY KEY, body TEXT NOT NULL, \
         version BIGINT NOT NULL)",
    )
    .await;
    sql(pool, "INSERT INTO caller_tx_docs VALUES (1, 'a', 0)").await;
}

async fn items(pool: &Pool) -> i64 {
    cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM caller_tx_items")
        .fetch_one(pool)
        .await
        .expect("count items")
}

fn item(id: i64, owner_row_id: i64) -> CreateCallerTxItemInput {
    CreateCallerTxItemInput {
        id,
        ownerRowId: owner_row_id,
    }
}

/// The create-policy probe runs on the caller's transaction, so it sees the
/// caller's own writes: a parent created earlier in it authorises the child,
/// and a parent handed to someone else earlier in it refuses the child.
#[tokio::test]
async fn policy_reads_see_the_callers_own_writes() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = caller();

    // A parent inserted earlier in the same transaction authorises its child.
    reset(pool).await;
    db.transaction(async |tx| {
        let owner = CreateCallerTxOwnerInput { id: 1, ownerId: 1 };
        db.caller_tx_owner()
            .create(owner)
            .run_in_tx(tx, &ctx)
            .await?;
        db.caller_tx_item()
            .create(item(1, 1))
            .run_in_tx(tx, &ctx)
            .await?;
        Ok(())
    })
    .await
    .expect("the probe sees the uncommitted parent");
    assert_eq!(items(pool).await, 1);

    // A parent handed to someone else earlier in the same transaction refuses
    // the child, and the whole transaction rolls back.
    reset(pool).await;
    sql(pool, "INSERT INTO caller_tx_owners VALUES (2, 1)").await;
    let handed_away = db
        .transaction(async |tx| {
            let transfer = UpdateCallerTxOwnerInput { ownerId: Some(99) };
            db.caller_tx_owner()
                .update(2)
                .set(transfer)
                .run_in_tx(tx, &ctx)
                .await?;
            db.caller_tx_item()
                .create(item(2, 2))
                .run_in_tx(tx, &ctx)
                .await?;
            Ok(())
        })
        .await;
    assert!(
        matches!(handed_away, Err(CratestackError::Forbidden(_))),
        "{handed_away:?}"
    );
    assert_eq!(items(pool).await, 0);

    // A later batch item is authorised by an earlier one.
    sql(pool, "INSERT INTO caller_tx_folders VALUES (1, 1, 1)").await;
    let batch = db
        .caller_tx_folder()
        .batch_create(vec![
            CreateCallerTxFolderInput {
                id: 2,
                ownerId: 1,
                parentId: 1,
            },
            CreateCallerTxFolderInput {
                id: 3,
                ownerId: 1,
                parentId: 2,
            },
        ])
        .run(&ctx)
        .await
        .expect("batch infrastructure");
    assert_eq!((batch.summary.ok, batch.summary.err), (2, 0));
}

/// The probe reads the caller's snapshot. Under `REPEATABLE READ` and
/// `SERIALIZABLE` a revocation another session commits after the snapshot was
/// taken is not seen and the write commits (a valid serial order places the
/// writer first); `READ COMMITTED` takes a fresh snapshot per statement and
/// refuses. The same table as `procedure_isolation_policy.rs`'s
/// `policy_reads_use_the_attempts_snapshot`.
#[tokio::test]
async fn policy_reads_use_the_callers_snapshot() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();

    for (level, committed) in [
        (TransactionIsolation::RepeatableRead, true),
        (TransactionIsolation::Serializable, true),
        (TransactionIsolation::ReadCommitted, false),
    ] {
        reset(pool).await;
        sql(pool, "INSERT INTO caller_tx_owners VALUES (3, 1)").await;
        let mut first_attempt = true;
        let result = run_in_isolated_tx(pool, level, |mut tx| {
            let (db, pool, ctx) = (db.clone(), pool.clone(), caller());
            let revoke = std::mem::replace(&mut first_attempt, false);
            async move {
                // Take the snapshot, then revoke from another session.
                cratestack::sqlx::query("SELECT 1")
                    .execute(&mut *tx)
                    .await
                    .map_err(cratestack::cratestack_error_from_sqlx)?;
                if revoke {
                    sql(
                        &pool,
                        "UPDATE caller_tx_owners SET owner_id = 99 WHERE id = 3",
                    )
                    .await;
                }
                db.caller_tx_item()
                    .create(item(3, 3))
                    .run_in_tx(&mut tx, &ctx)
                    .await?;
                Ok(((), tx))
            }
        })
        .await;
        if committed {
            assert!(result.is_ok(), "{level:?}: {result:?}");
        } else {
            assert!(
                matches!(result, Err(CratestackError::Forbidden(_))),
                "{level:?}: {result:?}"
            );
        }
        assert_eq!(items(pool).await, i64::from(committed), "{level:?}");
    }
}

/// A write in a caller's transaction needs one connection, not two: on a
/// one-connection pool a create with a relation policy finishes at once
/// instead of waiting out `acquire_timeout` for a second one.
#[tokio::test]
async fn a_write_in_a_callers_transaction_needs_one_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    sql(&test_pg.pool, "INSERT INTO caller_tx_owners VALUES (4, 1)").await;
    let one = one_connection_pool(&test_pg.url).await;
    let db = cratestack_schema::Cratestack::builder(one.clone()).build();
    let ctx = caller();
    let started = Instant::now();
    let result = db
        .transaction(async |tx| {
            db.caller_tx_item()
                .create(item(4, 4))
                .run_in_tx(tx, &ctx)
                .await?;
            Ok(())
        })
        .await;
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "{result:?} after {elapsed:?}");
    assert!(
        elapsed < BUDGET,
        "waited for a second connection: {elapsed:?}"
    );
    assert_eq!(items(&test_pg.pool).await, 1);
}

/// The same for `batch_create`: its create-policy probes run on the batch's
/// own transaction (a savepoint per item), not on the pool.
#[tokio::test]
async fn a_batch_create_needs_one_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    sql(pool, "INSERT INTO caller_tx_folders VALUES (1, 1, 1)").await;
    let one = one_connection_pool(&test_pg.url).await;
    let db = cratestack_schema::Cratestack::builder(one.clone()).build();
    let started = Instant::now();
    let batch = db
        .caller_tx_folder()
        .batch_create(vec![CreateCallerTxFolderInput {
            id: 2,
            ownerId: 1,
            parentId: 1,
        }])
        .run(&caller())
        .await
        .expect("batch infrastructure");
    let elapsed = started.elapsed();
    assert_eq!((batch.summary.ok, batch.summary.err), (1, 0), "{elapsed:?}");
    assert!(
        elapsed < BUDGET,
        "waited for a second connection: {elapsed:?}"
    );
}

/// The same for an upsert's update branch: `row_passes_update_policy` reads
/// the caller's transaction, so it needs no second connection even without a
/// relation in the policy.
#[tokio::test]
async fn an_upsert_in_a_callers_transaction_needs_one_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    sql(&test_pg.pool, "INSERT INTO caller_tx_owners VALUES (4, 1)").await;
    let one = one_connection_pool(&test_pg.url).await;
    let db = cratestack_schema::Cratestack::builder(one.clone()).build();
    let ctx = caller();
    let started = Instant::now();
    let result = db
        .transaction(async |tx| {
            db.caller_tx_owner()
                .upsert(CreateCallerTxOwnerInput { id: 4, ownerId: 1 })
                .run_in_tx(tx, &ctx)
                .await?;
            Ok(())
        })
        .await;
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "{result:?} after {elapsed:?}");
    assert!(
        elapsed < BUDGET,
        "waited for a second connection: {elapsed:?}"
    );
}

/// The downstream shape (vaam-apps/vaam-apps#507): more writers than pooled
/// connections, each holding a row lock and then writing under a relation
/// policy. Four writers on two connections: the lock holder used to need a
/// third connection for its probe while the other was held by a waiter
/// blocked on that very lock, so nothing progressed until the pool timed out.
#[tokio::test]
async fn row_locked_writers_past_the_pool_size_all_finish() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    sql(&test_pg.pool, "INSERT INTO caller_tx_owners VALUES (6, 1)").await;
    let two = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&test_pg.url)
        .await
        .expect("two-connection pool");
    let db = cratestack_schema::Cratestack::builder(two.clone()).build();
    let started = Instant::now();
    let writers = (0..4_i64).map(|i| {
        let (db, two, ctx) = (db.clone(), two.clone(), caller());
        async move {
            let mut tx = two.begin().await.map_err(|e| e.to_string())?;
            cratestack::sqlx::query("SELECT 1 FROM caller_tx_owners WHERE id = 6 FOR UPDATE")
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            db.caller_tx_item()
                .create(item(10 + i, 6))
                .run_in_tx(&mut tx, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())
        }
    });
    let results = join_all(writers).await;
    let elapsed = started.elapsed();
    assert!(
        results.iter().all(Result::is_ok),
        "{results:?} after {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "writers waited on the pool: {elapsed:?}"
    );
    assert_eq!(items(&test_pg.pool).await, 4);
}

/// An audited `.run()` with a relation policy, on a fresh runtime over a
/// one-connection pool: `.run()` holds its transaction's connection while the
/// create probe runs and while the audit bootstrap asks whether
/// `cratestack_audit` exists, and neither takes a second one.
#[tokio::test]
async fn an_audited_run_with_a_relation_policy_needs_one_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    cratestack::sqlx::raw_sql(cratestack::AUDIT_TABLE_DDL)
        .execute(&test_pg.pool)
        .await
        .expect("audit table");
    sql(&test_pg.pool, "INSERT INTO caller_tx_owners VALUES (7, 1)").await;
    let one = one_connection_pool(&test_pg.url).await;
    // Built here, so its `audit_table_ensured` flag starts false.
    let db = cratestack_schema::Cratestack::builder(one.clone()).build();
    let started = Instant::now();
    let result = db
        .caller_tx_audited()
        .create(CreateCallerTxAuditedInput {
            id: 7,
            ownerRowId: 7,
        })
        .run(&caller())
        .await;
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "{result:?} after {elapsed:?}");
    assert!(
        elapsed < BUDGET,
        "waited for a second connection: {elapsed:?}"
    );
    let audited: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM caller_tx_auditeds")
            .fetch_one(&test_pg.pool)
            .await
            .expect("count audited");
    assert_eq!(audited, 1);
}

/// The update/delete `@version` probe and the upsert update-policy check read
/// the caller's transaction too. After the caller bumps a row's version, a
/// second write with the old `If-Match` matches no row, and the probe, which
/// sees the bump, answers `412`, not `403`. After the caller hands a row to
/// another owner, an upsert of it is refused by the update gate.
#[tokio::test]
async fn version_probe_and_upsert_policy_read_the_callers_transaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = caller();
    reset(pool).await;
    reset_docs(pool).await;
    sql(pool, "INSERT INTO caller_tx_owners VALUES (5, 1)").await;

    let codes = db
        .transaction(async |tx| {
            let body = |text: &str| UpdateCallerTxDocInput {
                body: Some(text.to_owned()),
            };
            let code = |result: Result<(), CratestackError>| match result {
                Ok(()) => "ok".to_owned(),
                Err(error) => error.code().to_owned(),
            };
            db.caller_tx_doc()
                .update(1)
                .set(body("b"))
                .if_match(0)
                .run_in_tx(tx, &ctx)
                .await?;
            let stale_update = db
                .caller_tx_doc()
                .update(1)
                .set(body("c"))
                .if_match(0)
                .run_in_tx(tx, &ctx)
                .await
                .map(|_| ());
            let stale_removal = db
                .caller_tx_doc()
                .delete(1)
                .if_match(0)
                .run_in_tx(tx, &ctx)
                .await
                .map(|_| ());

            let transfer = UpdateCallerTxOwnerInput { ownerId: Some(99) };
            db.caller_tx_owner()
                .update(5)
                .set(transfer)
                .run_in_tx(tx, &ctx)
                .await?;
            let upsert = db
                .caller_tx_owner()
                .upsert(CreateCallerTxOwnerInput { id: 5, ownerId: 1 })
                .run_in_tx(tx, &ctx)
                .await
                .map(|_| ());
            Ok((code(stale_update), code(stale_removal), code(upsert)))
        })
        .await
        .expect("the caller transaction itself");
    assert_eq!(
        codes,
        (
            "PRECONDITION_FAILED".to_owned(),
            "PRECONDITION_FAILED".to_owned(),
            "FORBIDDEN".to_owned()
        ),
        "(stale update, stale delete, upsert of a row handed away)"
    );
}
