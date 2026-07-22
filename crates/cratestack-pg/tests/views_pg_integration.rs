//! Integration tests for `view` blocks (ADR-0003) against a real
//! Postgres. Exercises the full pipeline: schema → macro emit →
//! runtime → `find_many` / `find_unique` over a `CREATE VIEW`,
//! including `@@allow("read", ...)` policy enforcement.

use cratestack::include_server_schema;
use cratestack::sqlx::{PgPool, query};
use cratestack::{CoolContext, Value};

include_server_schema!("tests/fixtures/views_integration.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &PgPool) {
    // Order matters: the view depends on the table, so drop the view
    // first. `IF EXISTS` keeps the reset idempotent.
    query("DROP VIEW IF EXISTS active_customer_summarys")
        .execute(pool)
        .await
        .expect("drop view");
    query("DROP TABLE IF EXISTS view_customers")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE view_customers (
            id BIGINT PRIMARY KEY,
            email TEXT NOT NULL,
            active BOOLEAN NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create table");
    query(
        "CREATE VIEW active_customer_summarys AS \
         SELECT id, email FROM view_customers WHERE active = true",
    )
    .execute(pool)
    .await
    .expect("create view");
}

async fn seed_customers(pool: &PgPool) {
    query("INSERT INTO view_customers (id, email, active) VALUES (1, 'a@x.dev', true), (2, 'b@x.dev', false), (3, 'c@x.dev', true)")
        .execute(pool)
        .await
        .expect("seed");
}

fn authenticated() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::String("user-7".to_owned()))])
}

#[tokio::test]
async fn view_find_many_returns_projected_rows() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_customers(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = authenticated();

    let mut rows = cool
        .views()
        .active_customer_summary()
        .find_many()
        .run(&ctx)
        .await
        .expect("find_many returns ok");
    rows.sort_by_key(|row| row.id);

    assert_eq!(rows.len(), 2, "view filters out inactive customers");
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].email, "a@x.dev");
    assert_eq!(rows[1].id, 3);
    assert_eq!(rows[1].email, "c@x.dev");
}

#[tokio::test]
async fn view_find_unique_returns_single_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_customers(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = authenticated();

    let row = cool
        .views()
        .active_customer_summary()
        .find_unique(1)
        .run(&ctx)
        .await
        .expect("find_unique returns ok")
        .expect("active customer 1 exists");
    assert_eq!(row.email, "a@x.dev");

    let inactive = cool
        .views()
        .active_customer_summary()
        .find_unique(2)
        .run(&ctx)
        .await
        .expect("find_unique returns ok");
    assert!(
        inactive.is_none(),
        "inactive customer is filtered by the view's SQL body",
    );
}

#[tokio::test]
async fn view_allow_read_blocks_anonymous_callers() {
    // `@@allow("read", auth() != null)` should hide every row from
    // an unauthenticated caller.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_customers(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let anonymous = CoolContext::anonymous();

    let rows = cool
        .views()
        .active_customer_summary()
        .find_many()
        .run(&anonymous)
        .await
        .expect("find_many returns ok");
    assert!(
        rows.is_empty(),
        "@@allow('read', auth() != null) hides rows from anonymous callers",
    );
}
