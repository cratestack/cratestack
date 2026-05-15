//! End-to-end tests for tier-5 builder verbs against real Postgres:
//! JSONB `?` and `->>` operators via `FieldRef::json_has_key` /
//! `::json_get_text`.
//!
//! The tests seed raw JSONB via `sqlx::query` and verify behavior
//! through `aggregate().count()` — that avoids the model's
//! cratestack-tagged JSON decoder, which would otherwise refuse
//! to deserialize the test fixture's plain-shaped JSON. (The decoder
//! itself is a separate concern from the JSONB *operator* surface
//! this PR adds; tested via the matched-row counts.)

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, Value};
use support::pg;

include_server_schema!("tests/fixtures/builder_extensions_tier5.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS model_runs")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE model_runs (
            id BIGINT PRIMARY KEY,
            metrics JSONB
        )",
    )
    .execute(pool)
    .await
    .expect("create model_runs");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("tier5-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    let rows: &[(i64, Option<&str>)] = &[
        // (1) "loss" present with a value
        (1, Some(r#"{"loss": "0.001", "epoch": 5}"#)),
        // (2) "loss" key present but JSON null
        (2, Some(r#"{"loss": null, "epoch": 6}"#)),
        // (3) No "loss" key at all
        (3, Some(r#"{"epoch": 7}"#)),
        // (4) JSONB column itself null
        (4, None),
    ];
    for (id, json) in rows {
        let value: Option<cratestack::sqlx::types::Json<serde_json::Value>> = json
            .map(|s| cratestack::sqlx::types::Json(serde_json::from_str(s).unwrap()));
        query("INSERT INTO model_runs (id, metrics) VALUES ($1, $2)")
            .bind(id)
            .bind(value)
            .execute(pool)
            .await
            .expect("seed row");
    }
}

#[tokio::test]
async fn json_has_key_matches_present_keys_including_null_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::model_run;
    // PG `?` returns true when the top-level key exists, including
    // when the value is JSON null — so rows 1 and 2 match, row 3
    // (no `loss` key) does not, row 4 (null column) does not.
    let total: i64 = cool
        .model_run()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(model_run::metrics().json_has_key("loss"))
        .run()
        .await
        .expect("query succeeds");
    assert_eq!(total, 2);
}

#[tokio::test]
async fn json_get_text_eq_filters_by_extracted_string() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::model_run;
    let total: i64 = cool
        .model_run()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(model_run::metrics().json_get_text("loss").eq("0.001"))
        .run()
        .await
        .unwrap();
    assert_eq!(total, 1, "only row 1 has loss = '0.001'");
}

#[tokio::test]
async fn json_get_text_is_not_null_excludes_null_value_and_missing_key() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::model_run;
    // PG `->> 'loss'` returns NULL when the key is missing OR when
    // the JSON value is JSON null. So IS NOT NULL matches only row 1.
    let total: i64 = cool
        .model_run()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(model_run::metrics().json_get_text("loss").is_not_null())
        .run()
        .await
        .unwrap();
    assert_eq!(total, 1);
}

#[tokio::test]
async fn json_get_text_composes_with_other_predicates() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::model_run;
    let total: i64 = cool
        .model_run()
        .bind(ctx)
        .aggregate()
        .count()
        .where_(model_run::id().lt(3_i64))
        .where_expr(model_run::metrics().json_has_key("loss"))
        .run()
        .await
        .unwrap();
    assert_eq!(total, 2, "id<3 AND has 'loss' key");
}

#[tokio::test]
async fn json_has_key_preview_uses_question_operator() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    use cratestack_schema::model_run;
    let preview = cool
        .model_run()
        .find_many()
        .where_expr(model_run::metrics().json_has_key("loss"))
        .preview_sql();
    assert!(preview.contains("metrics ? $1"), "got: {preview}");
}
