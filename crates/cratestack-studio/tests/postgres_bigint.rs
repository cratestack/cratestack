//! A `BigInt` written through the Postgres source must bind as an integer
//! parameter (ADR 0019).
//!
//! `BigInt` is a canonical decimal string on every wire. The Postgres
//! source picks each bind from the schema's declared type; a `BigInt` that
//! fell through to the text bind would be sent as `TEXT` and refused by
//! the `BIGINT` column. A unit test can show which `TypedValue` is chosen;
//! only a live database shows the column accepts it and stores the exact
//! value. The DDL comes from the real Postgres emitter.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` or
//! `CRATESTACK_USE_TESTCONTAINERS=1` is set; `CRATESTACK_REQUIRE_DB=1`
//! turns the skip into a panic so a run cannot go green without having
//! run this for real.

use std::sync::Arc;

use cratestack_studio::data::DataSource;
use cratestack_studio::data::postgres::PostgresSource;
use cratestack_studio::data::{PageRequest, Row};
use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

mod support;

use support::pg;

const SCHEMA: &str = r#"
model StudioBigIntProbe {
  id BigInt @id
  amountE8 BigInt
  feeE8 BigInt?
}
"#;

/// Derived by `table_name("StudioBigIntProbe")`; distinctive so it cannot
/// collide with another test binary sharing the compose Postgres.
const TABLE: &str = "studio_big_int_probes";

/// Both `i64` bounds and the first integer a double cannot hold.
const BOUNDARIES: [(&str, i64); 3] = [
    ("9223372036854775807", i64::MAX),
    ("-9223372036854775808", i64::MIN),
    ("9007199254740993", 9_007_199_254_740_993),
];

async fn fixture(pool: &PgPool) -> PostgresSource {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema parses");
    let empty = cratestack_parser::parse_schema(
        "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n",
    )
    .expect("empty schema parses");
    let ddl = cratestack_migrate::emit::postgres::emit(
        &cratestack_migrate::diff(&empty, &schema).expect("diff"),
    )
    .up;
    for sql in [format!("DROP TABLE IF EXISTS \"{TABLE}\""), ddl] {
        // `AssertSqlSafe`: test-only DDL built from consts in this file
        // and the migrate emitter (sqlx 0.9's `SqlSafeStr` bound).
        sqlx_core::raw_sql::raw_sql(sqlx_core::sql_str::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("fixture ddl");
    }
    PostgresSource::new(pool.clone(), Arc::new(schema))
}

fn payload(id: &str, amount: &str) -> Row {
    let mut row = Row::new();
    row.insert("id".to_owned(), serde_json::json!(id));
    row.insert("amountE8".to_owned(), serde_json::json!(amount));
    row
}

async fn stored_amount(pool: &PgPool, id: i64) -> (String, i64) {
    let sql =
        format!("SELECT pg_typeof(amount_e8)::text, amount_e8 FROM \"{TABLE}\" WHERE id = $1");
    let row = sqlx_core::query::query(sqlx_core::sql_str::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row stored under the integer key");
    (row.get(0), row.get(1))
}

#[tokio::test]
async fn create_binds_bigint_as_an_integer_at_the_boundaries() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    for (text, expected) in BOUNDARIES {
        let created = source
            .create("StudioBigIntProbe", &payload(text, text))
            .await
            .unwrap_or_else(|error| panic!("create {text}: {error:?}"));
        assert_eq!(created["id"], serde_json::json!(expected), "{text}");

        let (pg_type, amount) = stored_amount(pool, expected).await;
        assert_eq!(pg_type, "bigint");
        assert_eq!(amount, expected);
    }
}

#[tokio::test]
async fn get_update_delete_and_cursor_work_on_a_bigint_key() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;
    for (text, _) in BOUNDARIES {
        source
            .create("StudioBigIntProbe", &payload(text, "1"))
            .await
            .expect("create");
    }

    // The `$1::bigint` cast on the key parameter is exact: a cast through
    // a double would round the largest key onto a neighbour.
    let row = source
        .get("StudioBigIntProbe", "9223372036854775807")
        .await
        .expect("get")
        .expect("row present");
    assert_eq!(row["id"], serde_json::json!(i64::MAX));

    let mut patch = Row::new();
    patch.insert("feeE8".to_owned(), serde_json::json!("9007199254740993"));
    let updated = source
        .update("StudioBigIntProbe", "9007199254740993", &patch)
        .await
        .expect("update")
        .expect("row present");
    assert_eq!(
        updated["feeE8"],
        serde_json::json!(9_007_199_254_740_993_i64)
    );

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = source
            .list(
                "StudioBigIntProbe",
                PageRequest {
                    cursor: cursor.as_deref(),
                    limit: Some(1),
                },
            )
            .await
            .expect("list");
        seen.extend(
            page.rows
                .iter()
                .map(|r| r["id"].as_i64().expect("integer id")),
        );
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, [i64::MIN, 9_007_199_254_740_993, i64::MAX]);

    let deleted = source
        .delete("StudioBigIntProbe", "-9223372036854775808")
        .await
        .expect("delete")
        .expect("row present");
    assert_eq!(deleted["id"], serde_json::json!(i64::MIN));
}

/// Defence in depth: the API validates before the source is reached, but
/// if a non-canonical string does get here it must not be turned into a
/// number (the old `Int` arm would have written `0`). It is bound as text
/// and Postgres refuses it, so nothing is stored.
#[tokio::test]
async fn a_non_canonical_bigint_is_refused_by_postgres_not_coerced() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let result = source
        .create("StudioBigIntProbe", &payload("1", "007"))
        .await;
    assert!(result.is_err(), "007 must be refused, got {result:?}");

    let count: i64 = sqlx_core::query_scalar::query_scalar(sqlx_core::sql_str::AssertSqlSafe(
        format!("SELECT count(*) FROM \"{TABLE}\""),
    ))
    .fetch_one(pool)
    .await
    .expect("count");
    assert_eq!(count, 0, "a refused write must not leave a row behind");
}
