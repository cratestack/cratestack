//! End-to-end test for column projection (`.select(...)`) against
//! real Postgres. Uses a codegen-driven fixture so the macro-emitted
//! `FromPartialPgRow` impl is exercised — not just hand-rolled code
//! like the in-memory rusqlite tests.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, Value};
use support::pg;

include_server_schema!(
    "tests/fixtures/builder_extensions_tier8.cstack",
    db = Postgres
);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS payment_intents")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE payment_intents (
            id BIGINT PRIMARY KEY,
            connector_id TEXT NOT NULL,
            status TEXT NOT NULL,
            amount_minor BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create payment_intents");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))]).with_request_id("tier8-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    let rows: &[(i64, &str, &str, i64)] = &[
        (1, "stripe_live", "succeeded", 1000),
        (2, "adyen_test", "pending", 250),
    ];
    for (id, connector, status, amount) in rows {
        query(
            "INSERT INTO payment_intents (id, connector_id, status, amount_minor) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(*connector)
        .bind(*status)
        .bind(amount)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn find_unique_select_populates_only_requested_columns() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::payment_intent;
    // Project just `connector_id` from a wide row — the named upstream
    // unblock site's pattern.
    let projection = cool
        .payment_intent()
        .bind(ctx)
        .find_unique(1)
        .select([payment_intent::connectorId().column_name()])
        .run()
        .await
        .expect("select succeeds")
        .expect("row exists");

    assert!(projection.is_selected("connector_id"));
    assert!(!projection.is_selected("status"));
    assert_eq!(projection.value.connectorId, "stripe_live");
    // status and amountMinor not selected → default values.
    assert_eq!(projection.value.status, "");
    assert_eq!(projection.value.amountMinor, 0);
}

#[tokio::test]
async fn find_many_select_projects_each_row_and_decodes_selected_fields() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::payment_intent;
    let projections = cool
        .payment_intent()
        .bind(ctx)
        .find_many()
        .order_by(payment_intent::id().asc())
        .select([
            payment_intent::connectorId().column_name(),
            payment_intent::amountMinor().column_name(),
        ])
        .run()
        .await
        .unwrap();

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].value.connectorId, "stripe_live");
    assert_eq!(projections[0].value.amountMinor, 1000);
    assert_eq!(projections[0].value.status, "", "status not in selection");
    assert_eq!(projections[1].value.connectorId, "adyen_test");
    assert_eq!(projections[1].value.amountMinor, 250);

    // Selection manifest is uniform across rows.
    assert!(projections.iter().all(|p| p.is_selected("connector_id")));
    assert!(projections.iter().all(|p| p.is_selected("amount_minor")));
    assert!(projections.iter().all(|p| !p.is_selected("status")));
}

#[tokio::test]
async fn find_unique_select_with_filter_under_read_policy_returns_none_when_denied() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // Anonymous context — read policy requires auth, so find_unique
    // returns None even though the row exists. This verifies the
    // projection still routes through `push_scoped_conditions` and
    // applies the read policy.
    let anon = CoolContext::anonymous();

    use cratestack_schema::payment_intent;
    let projection = cool
        .payment_intent()
        .bind(anon)
        .find_unique(1)
        .select([payment_intent::connectorId().column_name()])
        .run()
        .await
        .unwrap();
    assert!(projection.is_none(), "policy must deny anonymous lookups");
}

#[tokio::test]
async fn find_many_select_run_in_tx_locks_rows_and_returns_projection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::payment_intent;

    // Open a transaction explicitly so we can both (a) verify the
    // projection runs against the borrowed connection and (b) prove
    // the FOR UPDATE clause is appended without the query failing —
    // postgres lets FOR UPDATE only inside a transaction.
    let mut tx = pool.begin().await.expect("begin tx");
    let projections = cool
        .payment_intent()
        .bind(ctx)
        .find_many()
        .order_by(payment_intent::id().asc())
        .select([
            payment_intent::connectorId().column_name(),
            payment_intent::amountMinor().column_name(),
        ])
        .for_update()
        .run_in_tx(&mut tx)
        .await
        .expect("run_in_tx succeeds");
    tx.commit().await.expect("commit");

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].value.connectorId, "stripe_live");
    assert_eq!(projections[0].value.amountMinor, 1000);
    assert_eq!(projections[1].value.connectorId, "adyen_test");
    assert_eq!(projections[1].value.amountMinor, 250);
    assert!(projections.iter().all(|p| p.is_selected("connector_id")));
    assert!(projections.iter().all(|p| p.is_selected("amount_minor")));
    assert!(projections.iter().all(|p| !p.is_selected("status")));
}
