//! Integration test for materialized views (ADR-0003) against real
//! Postgres. Exercises the `refresh()` method on a `@@materialized`
//! view, asserting that:
//!
//! - rows inserted **before** `refresh()` are visible.
//! - rows inserted **after** the last `refresh()` are NOT visible
//!   until the next refresh — the materialized view holds a stale
//!   snapshot until explicitly told to recompute. This is the whole
//!   point of `@@materialized` vs a plain view.
//!
//! Uses `REFRESH MATERIALIZED VIEW CONCURRENTLY` per ADR-0003
//! §"Materialized views"; the unique-index DDL the macro generates
//! is what makes concurrent refresh possible.

use cratestack::include_server_schema;
use cratestack::sqlx::{PgPool, query};
use cratestack::{CoolContext, Value};

include_server_schema!("tests/fixtures/views_materialized.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &PgPool) {
    query("DROP MATERIALIZED VIEW IF EXISTS sale_totals")
        .execute(pool)
        .await
        .expect("drop materialized view");
    query("DROP TABLE IF EXISTS sales")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE sales (
            id BIGINT PRIMARY KEY,
            amount_cents BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create table");
    // The materialized view + the unique index on `id`. The unique
    // index is the precondition for `REFRESH ... CONCURRENTLY`.
    query("CREATE MATERIALIZED VIEW sale_totals AS SELECT id, amount_cents FROM sales")
        .execute(pool)
        .await
        .expect("create materialized view");
    query("CREATE UNIQUE INDEX sale_totals_pkey ON sale_totals (id)")
        .execute(pool)
        .await
        .expect("create unique index");
}

#[tokio::test]
async fn refresh_makes_new_rows_visible_on_materialized_view() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // `@@allow("read", auth() != null)` on the matview — supply a
    // populated context so the policy lets rows through. The refresh
    // semantics are what's under test, not the policy machinery.
    let ctx = CoolContext::authenticated([(
        "id".to_owned(),
        Value::String("user-1".to_owned()),
    )]);

    // Empty matview at this point — no rows in `sales` and no
    // refresh has run since CREATE MATERIALIZED VIEW.
    let initial = cool
        .views()
        .sale_total()
        .find_many()
        .run(&ctx)
        .await
        .expect("find_many initial");
    assert!(initial.is_empty(), "matview starts empty");

    // Insert into the source table — the matview snapshot is still
    // empty until `refresh()`.
    query("INSERT INTO sales (id, amount_cents) VALUES (1, 100), (2, 250)")
        .execute(pool)
        .await
        .expect("seed sales");

    let pre_refresh = cool
        .views()
        .sale_total()
        .find_many()
        .run(&ctx)
        .await
        .expect("find_many pre-refresh");
    assert!(
        pre_refresh.is_empty(),
        "matview is stale until refresh() — new source rows are NOT visible",
    );

    // Refresh — the matview now reflects the source table.
    cool.views()
        .sale_total()
        .refresh()
        .await
        .expect("refresh succeeds");

    let mut post_refresh = cool
        .views()
        .sale_total()
        .find_many()
        .run(&ctx)
        .await
        .expect("find_many post-refresh");
    post_refresh.sort_by_key(|row| row.id);
    assert_eq!(post_refresh.len(), 2, "matview reflects post-refresh state");
    assert_eq!(post_refresh[0].id, 1);
    assert_eq!(post_refresh[0].amountCents, 100);
    assert_eq!(post_refresh[1].id, 2);
    assert_eq!(post_refresh[1].amountCents, 250);
}
