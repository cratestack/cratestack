//! The one-time `cratestack_audit` bootstrap (`ensure_audit_table`).
//!
//! Since the `@isolation` fix (GHSA-r67q-4qqq-g9gm) the bootstrap first asks
//! the audited write's own transaction whether the audit objects exist, and
//! skips the DDL when they do. A table created by an operator's own migration
//! without the indexes `AUDIT_TABLE_DDL` declares must still get them, as it
//! did when the DDL always ran once per runtime.

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CratestackContext, Value};

include_server_schema!("tests/fixtures/banking_audit.cstack", db = Postgres);

mod support;

use support::pg;

const AUDIT_INDEXES: [&str; 3] = [
    "cratestack_audit_model_idx",
    "cratestack_audit_tenant_idx",
    "cratestack_audit_undelivered_idx",
];

#[tokio::test]
async fn a_hand_created_audit_table_still_gets_its_indexes() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, accounts")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE accounts (id BIGINT PRIMARY KEY, customer_email TEXT NOT NULL, \
         risk_score BIGINT NOT NULL, balance BIGINT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("create accounts");
    // The table exactly as the DDL creates it, minus every index.
    let table_only = cratestack::AUDIT_TABLE_DDL
        .split("CREATE INDEX")
        .next()
        .expect("DDL starts with the table");
    cratestack::sqlx::raw_sql(cratestack::sqlx::AssertSqlSafe(table_only.to_owned()))
        .execute(pool)
        .await
        .expect("create cratestack_audit without indexes");

    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = CratestackContext::authenticated([
        ("id".to_owned(), Value::Int(7)),
        ("role".to_owned(), Value::String("admin".to_owned())),
    ]);
    db.account()
        .create(cratestack_schema::CreateAccountInput {
            id: 1,
            customerEmail: "alice@example.com".to_owned(),
            riskScore: 1,
            balance: 1,
        })
        .run(&ctx)
        .await
        .expect("audited create");

    for index in AUDIT_INDEXES {
        let exists: bool = cratestack::sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(index)
            .fetch_one(pool)
            .await
            .expect("probe index");
        assert!(exists, "{index} was not created by the audit bootstrap");
    }
}
