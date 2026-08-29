//! Live check that Postgres will plan the *parameterised* statements
//! Studio actually runs.
//!
//! This can't be unit-tested. Studio hands Postgres the same SQL as the
//! real query — placeholders and all — over sqlx's extended query
//! protocol, and whether `EXPLAIN` accepts a statement with unbound-at-
//! parse-time `$1` slots (and infers their types from the `::text` /
//! `::bigint` casts the builders emit) is a property of the server, not
//! of any string we can assert on.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` (external PG,
//! `just test-pg`) or `CRATESTACK_USE_TESTCONTAINERS=1` (ephemeral
//! per-binary container) is set. `CRATESTACK_REQUIRE_DB=1` turns the
//! skip into a panic so a run can't go green without having run this
//! for real.

use std::sync::Arc;

use cratestack_studio::data::DataSource;
use cratestack_studio::data::SqlOp;
use cratestack_studio::data::postgres::PostgresSource;
use sqlx_postgres::PgPool;

mod support;

use support::pg;

/// `Int` primary key on purpose: `get` then renders `"probe_id" =
/// $1::bigint`, which is the harder inference case for EXPLAIN than a
/// plain text key.
const PROBE_SCHEMA: &str = r#"
model StudioExplainProbe {
  probeId Int @id
  label String
}
"#;

const TABLE: &str = "studio_explain_probes";

async fn fixture(pool: &PgPool) -> PostgresSource {
    for sql in [
        format!("DROP TABLE IF EXISTS \"{TABLE}\""),
        format!("CREATE TABLE \"{TABLE}\" (probe_id BIGINT PRIMARY KEY, label TEXT NOT NULL)"),
        format!("INSERT INTO \"{TABLE}\" VALUES (1, 'one'), (2, 'two')"),
    ] {
        // `AssertSqlSafe`: test-only fixture DDL built from consts in this
        // file (sqlx 0.9's `SqlSafeStr` bound).
        sqlx_core::query::query(sqlx_core::sql_str::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("fixture ddl");
    }
    let schema = Arc::new(cratestack_parser::parse_schema(PROBE_SCHEMA).expect("schema parses"));
    PostgresSource::new(pool.clone(), schema)
}

#[tokio::test]
async fn explain_plans_the_parameterised_list_query() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let plan = source
        .explain(SqlOp::List, "StudioExplainProbe", None)
        .await
        .expect("explain");
    let text = plan.text.expect("a plan, not a note");
    assert!(text.contains("cost="), "plan should carry costs: {text}");
    assert!(text.contains(TABLE), "{text}");
}

#[tokio::test]
async fn explain_plans_a_bigint_cast_primary_key_lookup() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let plan = source
        .explain(SqlOp::Get, "StudioExplainProbe", Some("1"))
        .await
        .expect("explain");
    let text = plan.text.expect("a plan, not a note");
    assert!(text.contains("cost="), "{text}");
}

/// Refusing to plan a mutation is enforced before the driver is
/// touched, so it holds on Postgres too — and the row survives.
#[tokio::test]
async fn explaining_a_delete_neither_plans_nor_deletes() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let plan = source
        .explain(SqlOp::Delete, "StudioExplainProbe", Some("1"))
        .await
        .expect("explain");
    assert!(plan.text.is_none(), "mutation was planned: {plan:?}");
    assert!(
        plan.note.expect("note").contains("read operations"),
        "note should say why"
    );

    let still_there = source
        .get("StudioExplainProbe", "1")
        .await
        .expect("get")
        .expect("row survives");
    assert_eq!(still_there["label"], "one");
}

#[tokio::test]
async fn explain_for_get_without_a_key_declines_rather_than_erroring() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let plan = source
        .explain(SqlOp::Get, "StudioExplainProbe", None)
        .await
        .expect("declining is Ok, not Err");
    assert!(plan.text.is_none());
    assert!(plan.note.expect("note").contains("pk="));
}
