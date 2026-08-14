//! End-to-end tests for tier-5 builder verbs against real Postgres:
//! JSONB `?` and `->>` operators via `FieldRef::json_has_key` /
//! `::json_get_text`.
//!
//! The `json_*` tests below seed raw JSONB via `sqlx::query` and verify
//! behavior through `aggregate().count()`, sidestepping the model's
//! generated `Json` decoder entirely — the JSONB *operator* surface this
//! file covers is a separate concern from decoding.
//!
//! `find_many_decodes_plain_shaped_jsonb_not_written_by_cratestack` and
//! `create_then_read_round_trips_json_column_as_plain_jsonb_on_disk` at
//! the bottom of the file *do* go through the decoder (and the write
//! path): they're the cratestack#162 regression coverage — before that
//! fix, the seeded plain-shaped JSON above (`{"loss": "0.001", ...}`,
//! not the externally-tagged `{"Map": {"loss": {"String": ...}}}` that
//! `Value` used to derive) failed to decode through the generated model
//! with a serde error, and a `Json` value written through the ORM was
//! persisted tagged rather than as plain JSON. `Value`'s derive has since
//! been replaced by untagged impls, so the two paths now agree.

mod support;

use std::collections::BTreeMap;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CratestackContext, Json, Value};
use support::pg;

include_server_schema!(
    "tests/fixtures/builder_extensions_tier5.cstack",
    db = Postgres
);

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

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
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
        let value: Option<cratestack::sqlx::types::Json<serde_json::Value>> =
            json.map(|s| cratestack::sqlx::types::Json(serde_json::from_str(s).unwrap()));
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

/// cratestack#162 regression (read side): rows here are seeded via raw
/// SQL with *plain* JSON (`seed()` above), never through cratestack's
/// own write path. Before the fix, `Json` fields decoded through
/// `sqlx::types::Json<Value>`, which deserializes via `Value`'s own
/// externally-tagged `Deserialize` — plain JSON like
/// `{"loss": "0.001", "epoch": 5}` doesn't match that shape at all, so
/// this failed with a serde error (`expected value at line 1 column
/// 2`-style), surfaced as a 500 on every `model.X.list`/`get` hitting a
/// row with legacy/external jsonb. This must decode cleanly now.
#[tokio::test]
async fn find_many_decodes_plain_shaped_jsonb_not_written_by_cratestack() {
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
    let rows = cool
        .model_run()
        .bind(ctx)
        .find_many()
        .order_by(model_run::id().asc())
        .run()
        .await
        .expect("decoding plain-shaped legacy jsonb must not fail");

    let mut expected_metrics = BTreeMap::new();
    expected_metrics.insert("loss".to_owned(), Value::String("0.001".to_owned()));
    expected_metrics.insert("epoch".to_owned(), Value::Int(5));
    assert_eq!(
        rows[0].metrics,
        Some(Json(Value::Map(expected_metrics))),
        "row 1's plain jsonb must decode to the equivalent typed Value, no variant tag involved"
    );

    assert_eq!(
        rows[1].metrics,
        Some(Json(Value::Map(BTreeMap::from([
            ("loss".to_owned(), Value::Null),
            ("epoch".to_owned(), Value::Int(6))
        ])))),
        "a JSON `null` value inside the object must decode as `Value::Null`, not fail"
    );

    assert_eq!(
        rows[3].metrics, None,
        "a NULL jsonb column must decode as `None`, not an error"
    );
}

/// cratestack#162 regression (write side): a `Json` column written
/// through the generated `Create...Input` must land on disk as *plain*
/// JSON, not `Value`'s own externally-tagged wire format. Native jsonb
/// operators (`->`/`->>`, exercised elsewhere in this file) and any
/// other reader of the column depend on that shape.
#[tokio::test]
async fn create_then_read_round_trips_json_column_as_plain_jsonb_on_disk() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let mut metrics = BTreeMap::new();
    metrics.insert("requests_per_second".to_owned(), Value::Int(5));
    let value = Value::Map(metrics);

    cool.model_run()
        .create(cratestack_schema::CreateModelRunInput {
            id: 100,
            metrics: Some(Json(value.clone())),
        })
        .run(&ctx)
        .await
        .expect("create with a non-empty Json column succeeds");

    let raw: (Option<serde_json::Value>,) =
        cratestack::sqlx::query_as("SELECT metrics FROM model_runs WHERE id = $1")
            .bind(100_i64)
            .fetch_one(pool)
            .await
            .expect("raw select succeeds");
    assert_eq!(
        raw.0,
        Some(serde_json::json!({ "requests_per_second": 5 })),
        "on-disk jsonb must be plain JSON, not `Value`'s externally-tagged encoding \
         (e.g. not `{{\"Map\": {{\"requests_per_second\": {{\"Int\": 5}}}}}}`)"
    );

    let fetched = cool
        .model_run()
        .find_unique(100)
        .run(&ctx)
        .await
        .expect("find_unique succeeds")
        .expect("row exists");
    assert_eq!(
        fetched.metrics,
        Some(Json(value)),
        "round trip through the ORM preserves the typed Value"
    );
}
