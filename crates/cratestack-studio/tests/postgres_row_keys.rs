//! End-to-end guard on the shape of rows the Postgres source returns.
//!
//! [`cratestack_studio::data::Row`] documents that row keys are
//! `.cstack` **field** names, not the snake_cased column names. The
//! SQLite source gets that for free (it labels its `json_object(...)`
//! pairs), but Postgres reaches the same contract only because the
//! projection aliases each column — and `row_to_json(t.*)` will happily
//! hand back column-named keys the moment that alias is dropped.
//!
//! A SQL-string unit test can prove the alias is *written*; only a live
//! database proves Postgres actually honours it through
//! `row_to_json`. Hence this file.
//!
//! **Every fixture field here is multi-word on purpose.** For a
//! single-word field (`id`, `status`) the camelCase field name and the
//! snake_case column name are identical, so a fixture built from those
//! passes whether or not the bug is present.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` is set — the
//! same convention every other PG-backed test in the workspace uses.
//! `just test-pg` sets it.

use std::sync::Arc;

use cratestack_studio::data::DataSource;
use cratestack_studio::data::postgres::PostgresSource;
use cratestack_studio::data::{PageRequest, Row};
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};

/// `probeId` is the primary key on purpose: cursor extraction looks the
/// PK up by *field* name, so a column-named row silently disables
/// pagination ("Next" never enables) on any model whose `@id` field is
/// multi-word.
const PROBE_SCHEMA: &str = r#"
model StudioRowKeyProbe {
  probeId String @id
  subjectId String
  jwkThumbprint String
  status String
}
"#;

/// Distinctive enough not to collide with another test binary sharing
/// the compose Postgres. Derived by `table_name("StudioRowKeyProbe")`.
const TABLE: &str = "studio_row_key_probes";

/// Every test in this binary rebuilds the same table, and cargo runs
/// them concurrently — without this they race in `CREATE TABLE` and
/// fail on `pg_type_typname_nsp_index` rather than on anything the test
/// is actually about.
async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

async fn connect_or_skip() -> Option<PgPool> {
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PoolOptions::<Postgres>::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()
}

async fn fixture(pool: &PgPool) -> PostgresSource {
    for sql in [
        format!("DROP TABLE IF EXISTS \"{TABLE}\""),
        format!(
            "CREATE TABLE \"{TABLE}\" (
               probe_id TEXT PRIMARY KEY,
               subject_id TEXT NOT NULL,
               jwk_thumbprint TEXT NOT NULL,
               status TEXT NOT NULL
             )"
        ),
        format!(
            "INSERT INTO \"{TABLE}\" VALUES
               ('p1', 'subj-alice', 'thumb-alice', 'active'),
               ('p2', 'subj-bob', 'thumb-bob', 'revoked')"
        ),
    ] {
        sqlx_core::query::query(&sql)
            .execute(pool)
            .await
            .expect("fixture ddl");
    }
    let schema = Arc::new(cratestack_parser::parse_schema(PROBE_SCHEMA).expect("schema parses"));
    PostgresSource::new(pool.clone(), schema)
}

fn assert_field_keyed(row: &Row) {
    let keys: Vec<&str> = row.keys().map(String::as_str).collect();
    for expected in ["probeId", "subjectId", "jwkThumbprint", "status"] {
        assert!(
            row.contains_key(expected),
            "missing '{expected}' in {keys:?}"
        );
    }
    for leaked in ["probe_id", "subject_id", "jwk_thumbprint"] {
        assert!(
            !row.contains_key(leaked),
            "column name '{leaked}' leaked into the row: {keys:?}"
        );
    }
}

#[tokio::test]
async fn list_returns_rows_keyed_by_cstack_field_names() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let source = fixture(&pool).await;

    let page = source
        .list("StudioRowKeyProbe", PageRequest::default())
        .await
        .expect("list");
    assert_eq!(page.rows.len(), 2);
    assert_field_keyed(&page.rows[0]);
    assert_eq!(page.rows[0]["subjectId"], "subj-alice");
}

#[tokio::test]
async fn get_returns_a_row_keyed_by_cstack_field_names() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let source = fixture(&pool).await;

    let row = source
        .get("StudioRowKeyProbe", "p2")
        .await
        .expect("get")
        .expect("row present");
    assert_field_keyed(&row);
    assert_eq!(row["jwkThumbprint"], "thumb-bob");
}

/// Cursor extraction reads the PK out of the returned row *by field
/// name*. With column-keyed rows the lookup misses and `next_cursor`
/// comes back `None`, which the UI renders as a permanently disabled
/// "Next" button — pagination silently dead on any multi-word PK.
#[tokio::test]
async fn pagination_cursor_survives_a_multi_word_primary_key() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let source = fixture(&pool).await;

    let page = source
        .list(
            "StudioRowKeyProbe",
            PageRequest {
                cursor: None,
                limit: Some(1),
            },
        )
        .await
        .expect("list");
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        page.next_cursor.as_deref(),
        Some("p1"),
        "cursor must come from the `probeId` field key"
    );

    let second = source
        .list(
            "StudioRowKeyProbe",
            PageRequest {
                cursor: page.next_cursor.as_deref(),
                limit: Some(1),
            },
        )
        .await
        .expect("second page");
    assert_eq!(second.rows[0]["probeId"], "p2");
}

/// The drawer repopulates its edit form from the row a write returns.
/// If that row is column-keyed, every multi-word field reads back as
/// `""` — and `build_payload` turns `""` on an optional field into an
/// explicit `null`, so the next save nulls columns the operator never
/// touched. Guarding the returned shape is what keeps that path honest.
#[tokio::test]
async fn update_returns_a_row_keyed_by_cstack_field_names() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let source = fixture(&pool).await;

    let mut payload = Row::new();
    payload.insert("status".to_owned(), serde_json::json!("suspended"));
    let row = source
        .update("StudioRowKeyProbe", "p1", &payload)
        .await
        .expect("update")
        .expect("row present");

    assert_field_keyed(&row);
    assert_eq!(row["status"], "suspended");
    // The fields the operator did not touch must still be readable —
    // this is exactly what the edit form re-snapshots.
    assert_eq!(row["subjectId"], "subj-alice");
    assert_eq!(row["jwkThumbprint"], "thumb-alice");
}
