mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, Value};
use cratestack::sqlx::Row;
use support::pg;

include_server_schema!("tests/fixtures/json_roundtrip.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS roundtrips").execute(pool).await.expect("drop table");
    query(
        "CREATE TABLE roundtrips (
            id BIGINT PRIMARY KEY,
            payload JSONB
        )",
    )
    .execute(pool)
    .await
    .expect("create roundtrip");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))]).with_request_id("json-rt-001")
}

#[tokio::test]
async fn create_and_read_roundtrip_json_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else { return; };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Build a payload: {"k": "v", "n": 1}
    let mut map = std::collections::BTreeMap::new();
    map.insert("k".to_string(), Value::String("v".to_string()));
    map.insert("n".to_string(), Value::Int(1));
    let payload = Value::Map(map);

    // Create via the generated API (this uses the write-side binding path).
    let created = cool
        .roundtrip()
        .create(cratestack_schema::CreateRoundtripInput { id: 42, payload: ::cratestack::Json(payload.clone()) })
        .run(&ctx)
        .await
        .expect("create succeeds");
    assert_eq!(created.id, 42);

    // Read back using generated find_unique (this exercises the decode path).
    let found = cool
        .roundtrip()
        .find_unique(42_i64)
        .run(&ctx)
        .await
        .expect("find_unique");

    let found = found.expect("row found");
    assert_eq!(found.payload.0, payload);

    // Also verify the raw JSONB in the DB is plain JSON (not tagged enum).
    let row = query("SELECT payload FROM roundtrips WHERE id = $1")
        .bind(42_i64)
        .fetch_one(pool)
        .await
        .expect("fetch raw");
    let raw: serde_json::Value = row.get("payload");
    // The raw should be a JSON object with keys k and n
    assert!(raw.get("k").is_some());
}
