//! A `BigInt` written through the SQLite source must land as an SQLite
//! INTEGER (ADR 0019).
//!
//! The embedded runtime reads a `BigInt` column back as an `i64`, and the
//! migrate emitter declares every SQLite column `BLOB` (no affinity), so a
//! TEXT value is stored as-is and only fails later, on the embedded read.
//! The DDL here comes from the real SQLite emitter to keep that true.
//!
//! No Docker: an on-disk SQLite file, so a second connection can ask
//! `typeof(...)` after the source has written.

use std::sync::Arc;

use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::data::{DataSource, PageRequest, Row};
use rusqlite::Connection;
use tempfile::TempDir;

const SCHEMA: &str = r#"
model Ledger {
  id BigInt @id
  amountE8 BigInt
  feeE8 BigInt?
}
"#;

/// Both `i64` bounds and the first integer a double cannot hold.
const BOUNDARIES: [(&str, i64); 3] = [
    ("9223372036854775807", i64::MAX),
    ("-9223372036854775808", i64::MIN),
    ("9007199254740993", 9_007_199_254_740_993),
];

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

fn payload(id: &str, amount: &str) -> Row {
    let mut row = Row::new();
    row.insert("id".to_owned(), serde_json::json!(id));
    row.insert("amountE8".to_owned(), serde_json::json!(amount));
    row
}

#[tokio::test]
async fn create_stores_bigint_values_as_sqlite_integers_at_the_boundaries() {
    let (_dir, source, inspector) = fixture();

    for (text, expected) in BOUNDARIES {
        source
            .create("Ledger", &payload(text, text))
            .await
            .unwrap_or_else(|error| panic!("create {text}: {error:?}"));

        let (id_type, amount_type, id, amount): (String, String, i64, i64) = inspector
            .query_row(
                "SELECT typeof(id), typeof(amount_e8), id, amount_e8 FROM ledgers WHERE id = ?1",
                [expected],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap_or_else(|error| panic!("{text} was not stored as an integer key: {error}"));
        assert_eq!(id_type, "integer", "{text}: key stored as {id_type}");
        assert_eq!(
            amount_type, "integer",
            "{text}: value stored as {amount_type}"
        );
        assert_eq!((id, amount), (expected, expected));
    }
}

#[tokio::test]
async fn get_update_and_cursor_pagination_work_on_a_bigint_key() {
    let (_dir, source, _inspector) = fixture();
    for (text, _) in BOUNDARIES {
        source
            .create("Ledger", &payload(text, "1"))
            .await
            .expect("create");
    }

    // Exact lookup of the largest key: a cast through a double would
    // round it onto a neighbour and miss.
    let row = source
        .get("Ledger", "9223372036854775807")
        .await
        .expect("get")
        .expect("row present");
    assert_eq!(row["id"], serde_json::json!(i64::MAX));

    let mut patch = Row::new();
    patch.insert("feeE8".to_owned(), serde_json::json!("9007199254740993"));
    let updated = source
        .update("Ledger", "9007199254740993", &patch)
        .await
        .expect("update")
        .expect("row present");
    assert_eq!(
        updated["feeE8"],
        serde_json::json!(9_007_199_254_740_993_i64)
    );

    // Ascending by key, one row per page, cursor re-bound as an integer.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = source
            .list(
                "Ledger",
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
}
