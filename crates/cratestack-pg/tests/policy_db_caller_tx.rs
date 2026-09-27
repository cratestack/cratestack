//! Where policy reads run when a write executes inside a transaction the
//! caller opened *without* `@isolation`: `db.transaction(..)`, `run_in_tx`,
//! `run_in_isolated_tx` and `batch_create`. They run on the pool — a second
//! connection, outside the caller's transaction — exactly as on `origin/main`
//! (6cbdb382). The `@isolation` fix (GHSA-r67q-4qqq-g9gm) moves them onto the
//! transaction only for a procedure's own attempt; its counterpart is
//! `procedure_isolation_policy.rs` (docs/design/procedure-isolation.md §4.1).
//!
//! These are not endorsements. Each case below is a known, pre-existing
//! limitation of pool-side policy reads, pinned so that any change to it is
//! deliberate: this file passes unchanged on `origin/main`.

use std::time::{Duration, Instant};

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    CratestackContext, CratestackError, TransactionIsolation, Value, run_in_isolated_tx,
};

include_server_schema!("tests/fixtures/policy_caller_tx.cstack", db = Postgres);

mod support;

use cratestack_schema::{CreateCallerTxFolderInput, CreateCallerTxItemInput};
use cratestack_schema::{
    CreateCallerTxOwnerInput, UpdateCallerTxDocInput, UpdateCallerTxOwnerInput,
};
use support::pg;

type Pool = cratestack::sqlx::PgPool;

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
         caller_tx_docs",
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

/// The create-policy probe reads committed data on a second connection, so
/// it cannot see the caller's own uncommitted writes.
#[tokio::test]
async fn policy_reads_do_not_see_the_callers_own_writes() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = caller();

    // A parent inserted earlier in the same transaction does not authorise
    // its child: the probe cannot see the uncommitted parent.
    reset(pool).await;
    let unseen_parent = db
        .transaction(async |tx| {
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
        .await;
    assert!(
        matches!(unseen_parent, Err(CratestackError::Forbidden(_))),
        "{unseen_parent:?}"
    );
    assert_eq!(items(pool).await, 0);

    // A parent handed to someone else earlier in the same transaction still
    // authorises the child: the probe reads the committed owner, and the
    // child commits under a parent the caller no longer owns.
    reset(pool).await;
    sql(pool, "INSERT INTO caller_tx_owners VALUES (2, 1)").await;
    db.transaction(async |tx| {
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
    .await
    .expect("the probe reads the committed owner");
    assert_eq!(items(pool).await, 1);

    // A later batch item is not authorised by an earlier one.
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
    assert_eq!((batch.summary.ok, batch.summary.err), (1, 1));
}

/// Under every level the probe reads the latest committed data, not the
/// caller's snapshot: a revocation another session commits after the
/// snapshot was taken refuses the write.
#[tokio::test]
async fn policy_reads_ignore_the_callers_snapshot() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();

    for level in [
        TransactionIsolation::RepeatableRead,
        TransactionIsolation::Serializable,
        TransactionIsolation::ReadCommitted,
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
        assert!(
            matches!(result, Err(CratestackError::Forbidden(_))),
            "{level:?}: {result:?}"
        );
        assert_eq!(items(pool).await, 0, "{level:?}");
    }
}

/// The probe needs a second pooled connection while the caller's transaction
/// holds one. On a one-connection pool it waits out `acquire_timeout`; with
/// `N` concurrent such writers on an `N`-connection pool nothing progresses
/// until then. The same holds for an audited or emitting `.run()`, whose
/// framework transaction holds a connection while its policy reads take
/// another. A known limitation for callers without `@isolation`.
#[tokio::test]
async fn policy_reads_need_a_second_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    sql(&test_pg.pool, "INSERT INTO caller_tx_owners VALUES (4, 1)").await;
    let one = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(500))
        .connect(&test_pg.url)
        .await
        .expect("one-connection pool");
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
    assert!(result.is_err(), "{result:?}");
    assert!(
        elapsed >= Duration::from_millis(450),
        "failed without waiting for a second connection: {elapsed:?} {result:?}"
    );
    assert_eq!(items(&test_pg.pool).await, 0);
}

/// The update/delete `@version` probe and the upsert update-policy check read
/// committed data on the pool too. Inside one caller transaction: after the
/// caller bumps a row's version, a second write with the old `If-Match`
/// matches no row, and the probe, which cannot see the bump, finds the
/// committed version equal to `If-Match` and answers `403`, not `412`. After
/// the caller hands a row to another owner, an upsert of it is still
/// authorised by the committed owner.
#[tokio::test]
async fn version_probe_and_upsert_policy_read_the_pool() {
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
            "FORBIDDEN".to_owned(),
            "FORBIDDEN".to_owned(),
            "ok".to_owned()
        ),
        "(stale update, stale delete, upsert of a row handed away)"
    );
}
