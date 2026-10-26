//! An ordinal enum (`Pending = 1`, `docs/design/enum-ordinals.md`)
//! written through the Postgres source lands in its `SMALLINT` column as
//! the number, whichever form the payload uses: the variant's name (what
//! the UI's `<select>` sends) or the ordinal. The DDL comes from the real
//! Postgres emitter, `CHECK` included, so a wrong number is refused by
//! the database too.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` or
//! `CRATESTACK_USE_TESTCONTAINERS=1` is set; `CRATESTACK_REQUIRE_DB=1`
//! turns the skip into a panic.

use std::sync::Arc;

use cratestack_studio::data::DataSource;
use cratestack_studio::data::Row;
use cratestack_studio::data::postgres::PostgresSource;
use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

mod support;

use support::pg;

const SCHEMA: &str = r#"
enum StudioProbeStatus {
  Pending = 1
  Approved = 2
  Refunded = 5

  @@reserved(3, 4)
}

model StudioOrdinalProbe {
  id Int @id
  status StudioProbeStatus @default(Pending)
  previous StudioProbeStatus?
}
"#;

/// `table_name("StudioOrdinalProbe")`, distinctive so it cannot collide
/// with another test binary sharing the compose Postgres.
const TABLE: &str = "studio_ordinal_probes";

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

fn row(items: &[(&str, serde_json::Value)]) -> Row {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

async fn stored(pool: &PgPool, id: i64) -> (String, i16, Option<i16>) {
    let sql =
        format!("SELECT pg_typeof(status)::text, status, previous FROM \"{TABLE}\" WHERE id = $1");
    let found = sqlx_core::query::query(sqlx_core::sql_str::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row stored");
    (found.get(0), found.get(1), found.get(2))
}

#[tokio::test]
async fn a_name_or_an_ordinal_is_stored_as_the_smallint() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    let source = fixture(pool).await;

    let by_name = row(&[("id", 1.into()), ("status", "Approved".into())]);
    let created = source
        .create("StudioOrdinalProbe", &by_name)
        .await
        .expect("create by name");
    assert_eq!(created["status"], serde_json::json!(2));
    assert_eq!(stored(pool, 1).await, ("smallint".to_owned(), 2, None));

    let by_ordinal = row(&[
        ("id", 2.into()),
        ("status", 5.into()),
        ("previous", serde_json::Value::Null),
    ]);
    source
        .create("StudioOrdinalProbe", &by_ordinal)
        .await
        .expect("create by ordinal, with a NULL optional");
    assert_eq!(stored(pool, 2).await.1, 5);

    let patch = row(&[("previous", "Pending".into())]);
    source
        .update("StudioOrdinalProbe", "2", &patch)
        .await
        .expect("update")
        .expect("row present");
    assert_eq!(stored(pool, 2).await.2, Some(1));

    // The column's own default is the ordinal of `@default(Pending)`.
    source
        .create("StudioOrdinalProbe", &row(&[("id", 3.into())]))
        .await
        .expect("create with the default");
    assert_eq!(stored(pool, 3).await.1, 1);
}
