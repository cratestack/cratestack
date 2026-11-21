//! cratestack#745: the one-time `cratestack_audit` bootstrap keeps the
//! server's SQLSTATE when its DDL fails. The runtime's pool is read-only, so
//! the DDL is the first thing to fail (the bootstrap runs before the insert).

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/banking_audit.cstack", db = Postgres);

mod support;

use support::failing_pools::{READ_ONLY_SQLSTATE, assert_typed, ctx, read_only_pool};
use support::pg;

#[tokio::test]
async fn audit_table_bootstrap_failure_keeps_its_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::raw_sql(
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, accounts;
         CREATE TABLE accounts (id BIGINT PRIMARY KEY, customer_email TEXT NOT NULL,
             risk_score BIGINT NOT NULL, balance BIGINT NOT NULL);",
    )
    .execute(&test_pg.pool)
    .await
    .expect("reset");

    let db = cratestack_schema::Cratestack::builder(read_only_pool(&test_pg.url).await).build();
    let err = db
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 1,
            customerEmail: "a@example.com".to_owned(),
            riskScore: 1,
            balance: 1,
        })
        .run(&ctx())
        .await
        .unwrap_err();

    assert_typed(&err, READ_ONLY_SQLSTATE);
    let audit_exists: bool =
        cratestack::sqlx::query_scalar("SELECT to_regclass('cratestack_audit') IS NOT NULL")
            .fetch_one(&test_pg.pool)
            .await
            .expect("probe");
    assert!(!audit_exists, "the bootstrap failed, so no audit table");
}

/// The probe that asks whether the audit objects exist runs on the audited
/// write's own transaction. When the server ends that session (idle in
/// transaction) the probe's error keeps the server's SQLSTATE.
#[tokio::test]
async fn audit_probe_failure_keeps_its_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    cratestack::sqlx::raw_sql(
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, accounts;
         CREATE TABLE accounts (id BIGINT PRIMARY KEY, customer_email TEXT NOT NULL,
             risk_score BIGINT NOT NULL, balance BIGINT NOT NULL);",
    )
    .execute(&test_pg.pool)
    .await
    .expect("reset");

    let options: cratestack::sqlx::postgres::PgConnectOptions =
        test_pg.url.parse().expect("url parses");
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.options([("idle_in_transaction_session_timeout", "200")]))
        .await
        .expect("pool connects");
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let mut tx = pool.begin().await.expect("begin");
    cratestack::sqlx::query("SELECT 1")
        .execute(&mut *tx)
        .await
        .expect("open the transaction");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let err = db
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 1,
            customerEmail: "a@example.com".to_owned(),
            riskScore: 1,
            balance: 1,
        })
        .run_in_tx(&mut tx, &ctx())
        .await
        .map(|_| ())
        .unwrap_err();
    println!("probe error: {err:?}");
    assert_typed(&err, "25P03");
}
