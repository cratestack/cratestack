//! End-to-end test for `@@soft_delete` + `@@retain`.
//!
//! Confirms that DELETE becomes UPDATE-of-`deleted_at`, that subsequent
//! reads filter the tombstoned row out, and that the row is still
//! physically present in PG (so banks can run their retention GC against
//! the recorded `retention_days` policy).

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value};

include_server_schema!("tests/fixtures/banking_soft_delete.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, customers")
        .execute(pool)
        .await
        .expect("drop");
    query(
        "CREATE TABLE customers (
            id BIGINT PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL,
            deleted_at TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create customer");
}

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn delete_tombstones_the_row_instead_of_removing_it() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO customers (id, name, email) VALUES (1, 'Alice', 'alice@example.com')")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    cool.customer().delete(1).run(&ctx()).await.expect("delete");

    // Row still physically there, but `deleted_at` is now non-null.
    let row = query("SELECT id, name, deleted_at FROM customers WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read raw");
    let id: i64 = row.get("id");
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("deleted_at").ok();
    assert_eq!(id, 1);
    assert!(
        deleted_at.is_some(),
        "soft-delete must set deleted_at to NOW(), got NULL",
    );
}

#[tokio::test]
async fn reads_filter_out_tombstoned_rows_in_find_unique() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO customers (id, name, email, deleted_at) \
         VALUES (1, 'Alive', 'a@x.io', NULL), (2, 'Gone', 'g@x.io', NOW() - INTERVAL '1 day')",
    )
    .execute(pool)
    .await
    .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let alive = cool
        .customer()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("alive lookup");
    assert!(alive.is_some(), "non-deleted row must be visible");

    let gone = cool
        .customer()
        .find_unique(2)
        .run(&ctx())
        .await
        .expect("gone lookup");
    assert!(
        gone.is_none(),
        "soft-deleted row must be filtered out by find_unique",
    );
}

#[tokio::test]
async fn reads_filter_out_tombstoned_rows_in_find_many() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO customers (id, name, email, deleted_at) VALUES \
            (1, 'A', 'a@x.io', NULL), \
            (2, 'B', 'b@x.io', NOW()), \
            (3, 'C', 'c@x.io', NULL)",
    )
    .execute(pool)
    .await
    .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let rows = cool.customer().find_many().run(&ctx()).await.expect("list");
    let mut ids = rows.iter().map(|c| c.id).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec![1, 3], "find_many must exclude soft-deleted rows",);
}

#[tokio::test]
async fn redeleting_a_tombstoned_row_does_not_change_the_timestamp_again() {
    // Banks want re-delete to be idempotent-ish: re-issuing DELETE on an
    // already-tombstoned row should either be a no-op or a clean failure,
    // not a moving timestamp that confuses audit. Today the runner refuses
    // because the WHERE clause requires `deleted_at IS NULL`.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO customers (id, name, email, deleted_at) \
         VALUES (1, 'Gone', 'g@x.io', NOW() - INTERVAL '1 day')",
    )
    .execute(pool)
    .await
    .expect("seed");
    let original: chrono::DateTime<chrono::Utc> =
        query("SELECT deleted_at FROM customers WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("read")
            .get("deleted_at");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let result = cool.customer().delete(1).run(&ctx()).await;
    assert!(
        result.is_err(),
        "re-deleting a tombstoned row should fail rather than silently bump the timestamp",
    );

    let after: chrono::DateTime<chrono::Utc> =
        query("SELECT deleted_at FROM customers WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("read")
            .get("deleted_at");
    assert_eq!(
        original, after,
        "deleted_at must not change on re-delete attempts",
    );
}
