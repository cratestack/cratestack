//! cratestack#745: `ViewDelegate::refresh` keeps the server's SQLSTATE and
//! reports a lost connection as `Unavailable`.

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/views_materialized.cstack", db = Postgres);

mod support;

use support::failing_pools::{
    READ_ONLY_SQLSTATE, assert_typed, assert_unavailable, closed_pool, read_only_pool,
};
use support::pg;

#[tokio::test]
async fn materialized_view_refresh_failures_are_typed() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::raw_sql(
        "DROP MATERIALIZED VIEW IF EXISTS sale_totals;
         DROP TABLE IF EXISTS sales;
         CREATE TABLE sales (id BIGINT PRIMARY KEY, amount_cents BIGINT NOT NULL);
         CREATE MATERIALIZED VIEW sale_totals AS SELECT id, amount_cents FROM sales;
         CREATE UNIQUE INDEX sale_totals_pkey ON sale_totals (id);",
    )
    .execute(&test_pg.pool)
    .await
    .expect("reset");

    let read_only =
        cratestack_schema::Cratestack::builder(read_only_pool(&test_pg.url).await).build();
    let err = read_only.views().sale_total().refresh().await.unwrap_err();
    assert_typed(&err, READ_ONLY_SQLSTATE);

    let closed = cratestack_schema::Cratestack::builder(closed_pool(&test_pg.url).await).build();
    let err = closed.views().sale_total().refresh().await.unwrap_err();
    assert_unavailable(&err);
}
