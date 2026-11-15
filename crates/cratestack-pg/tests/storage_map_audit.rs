//! `@@audit` on a model with mapped columns (cratestack#1375): the descriptor
//! holds SQL names (`ssn_enc`, `bank_id`) but the audit snapshot is keyed by
//! field name (`ssn`, `id`). Redaction and the primary key must translate, or a
//! mapped `@pii` value is logged in clear and the primary key is recorded null.

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CratestackContext, Value};

include_server_schema!("tests/fixtures/storage_map.cstack", db = Postgres);

mod storage_map_support;
mod support;

use storage_map_support::{reset_schema, seed};
use support::pg;

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn mapped_pii_and_sensitive_columns_are_redacted_and_the_primary_key_recorded() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.map_bank()
        .create(cratestack_schema::CreateMapBankInput {
            id: 7,
            code: "AAA".to_owned(),
            displayName: "Alpha".to_owned(),
            ssn: "123-45-6789".to_owned(),
            taxRef: "TAX-SECRET".to_owned(),
            region: "eu".to_owned(),
        })
        .run(&ctx)
        .await
        .expect("create");
    cool.map_bank()
        .update(7)
        .set(cratestack_schema::UpdateMapBankInput {
            code: None,
            displayName: None,
            ssn: Some("987-65-4321".to_owned()),
            taxRef: None,
            region: None,
        })
        .if_match(0)
        .run(&ctx)
        .await
        .expect("update");

    let rows = query(
        "SELECT operation, primary_key, before, after FROM cratestack_audit ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("audit rows");
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let pk: serde_json::Value = row.get("primary_key");
        assert!(
            pk == serde_json::json!(7) || pk == serde_json::json!("7"),
            "the mapped @id must not log a null primary key, got {pk}"
        );
    }
    let text = |value: &serde_json::Value| value.to_string();
    for column in ["before", "after"] {
        for row in &rows {
            let snapshot: Option<serde_json::Value> = row.get(column);
            let Some(snapshot) = snapshot else { continue };
            assert_eq!(snapshot["ssn"], serde_json::json!("[redacted-pii]"));
            assert_eq!(
                snapshot["taxRef"],
                serde_json::json!("[redacted-sensitive]")
            );
            assert_eq!(snapshot["displayName"], serde_json::json!("Alpha"));
            let dump = text(&snapshot);
            for secret in ["123-45-6789", "987-65-4321", "TAX-SECRET"] {
                assert!(!dump.contains(secret), "{secret} leaked into the audit log");
            }
        }
    }
}

#[tokio::test]
async fn deleting_a_mapped_row_records_its_primary_key() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    cool.map_bank()
        .delete(2)
        .if_match(0)
        .run(&operator())
        .await
        .expect("delete");
    let row = query("SELECT operation, primary_key, before FROM cratestack_audit")
        .fetch_one(pool)
        .await
        .expect("one audit row");
    assert_eq!(row.get::<String, _>("operation"), "delete");
    let pk: serde_json::Value = row.get("primary_key");
    assert!(
        pk == serde_json::json!(2) || pk == serde_json::json!("2"),
        "{pk}"
    );
    let before: serde_json::Value = row.get("before");
    assert_eq!(before["ssn"], serde_json::json!("[redacted-pii]"));
}
