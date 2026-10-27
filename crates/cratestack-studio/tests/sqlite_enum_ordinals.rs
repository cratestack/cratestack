//! An ordinal enum (`Pending = 1`, `docs/design/enum-ordinals.md`)
//! written through the SQLite source is stored as an SQLite INTEGER,
//! whichever form the payload uses. The embedded runtime reads the column
//! back as an integer, and the emitted column is `BLOB` (no affinity), so
//! a name stored as TEXT would only fail later, on the embedded read. The
//! DDL comes from the real SQLite emitter.

use std::sync::Arc;

use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::data::{DataSource, Row};
use rusqlite::Connection;
use tempfile::TempDir;

const SCHEMA: &str = r#"
enum Status {
  Pending = 1
  Approved = 2
  Refunded = 5
}

model Payment {
  id Int @id
  status Status
  previous Status?
}
"#;

fn fixture() -> (TempDir, SqliteSource, Connection) {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema parses");
    let empty = cratestack_parser::parse_schema("").expect("empty schema parses");
    let ddl = cratestack_migrate::emit::sqlite::emit(
        &cratestack_migrate::diff(&empty, &schema).expect("diff"),
    )
    .up;
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("studio.sqlite");
    let writer = Connection::open(&path).expect("open writer");
    writer.execute_batch(&ddl).expect("apply emitted ddl");
    let inspector = Connection::open(&path).expect("open inspector");
    (dir, SqliteSource::new(writer, Arc::new(schema)), inspector)
}

fn row(items: &[(&str, serde_json::Value)]) -> Row {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn stored(inspector: &Connection, id: i64) -> (String, i64, Option<i64>) {
    inspector
        .query_row(
            "SELECT typeof(status), status, previous FROM payments WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("row stored")
}

#[tokio::test]
async fn a_name_or_an_ordinal_is_stored_as_an_integer() {
    let (_dir, source, inspector) = fixture();

    let created = source
        .create(
            "Payment",
            &row(&[("id", 1.into()), ("status", "Approved".into())]),
        )
        .await
        .expect("create by name");
    assert_eq!(created["status"], serde_json::json!(2));
    assert_eq!(stored(&inspector, 1), ("integer".to_owned(), 2, None));

    source
        .create("Payment", &row(&[("id", 2.into()), ("status", 5.into())]))
        .await
        .expect("create by ordinal");
    source
        .update("Payment", "2", &row(&[("previous", "Pending".into())]))
        .await
        .expect("update")
        .expect("row present");
    assert_eq!(stored(&inspector, 2), ("integer".to_owned(), 5, Some(1)));
}
