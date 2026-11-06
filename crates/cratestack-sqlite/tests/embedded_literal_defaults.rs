//! cratestack#1234: the embedded runtime binds literal `@default`s on
//! create, and `create_table_sql` writes the same values as `DEFAULT`
//! clauses for rows inserted with raw SQL.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use cratestack::{RusqliteRuntime, RustDecimal, include_embedded_schema};
use cratestack_core::BatchItemStatus;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};
use uuid::Uuid;

include_embedded_schema!(
    "tests/fixtures/literal_defaults.cstack",
    decimal = RustDecimal
);

use cratestack_schema::models::Item;
use cratestack_schema::types::{Priority, Status};
use cratestack_schema::{CreateItemInput, ITEM_MODEL};

fn runtime(ddl: &str) -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn input(id: &str) -> CreateItemInput {
    CreateItemInput {
        id: id.to_owned(),
        title: format!("title {id}"),
    }
}

fn assert_defaults(item: &Item) {
    assert!(!item.done);
    assert_eq!(item.count, -3);
    assert_eq!(item.big.get(), 9_007_199_254_740_993);
    assert_eq!(item.ratio, 1.5);
    assert_eq!(item.price, RustDecimal::from_str("12.50").unwrap());
    assert_eq!(item.label, r#"it's "x""#);
    assert_eq!(item.status, Status::Live);
    assert_eq!(item.priority, Priority::High);
    assert_eq!(
        item.r#ref,
        Uuid::parse_str("6f1c2d4e-8b3a-4c5d-9e7f-0a1b2c3d4e5f").unwrap()
    );
    let since: DateTime<Utc> = "2026-01-02T03:04:05Z".parse().unwrap();
    assert_eq!(item.since, since);
    assert_eq!(item.note.as_deref(), Some("n/a"));
}

#[test]
fn create_batch_create_upsert_and_batch_upsert_bind_the_literals() {
    let runtime = runtime(&create_table_sql(&ITEM_MODEL));
    let items = ModelDelegate::<Item, String>::new(&runtime, &ITEM_MODEL);

    assert_defaults(&items.create(input("a")).run().expect("create"));
    assert_defaults(&items.upsert(input("b")).run().expect("upsert"));
    let created = items
        .batch_create(vec![input("c"), input("d")])
        .run()
        .expect("batch create");
    let upserted = items
        .batch_upsert(vec![input("e")])
        .run()
        .expect("batch upsert");
    for result in created.results.iter().chain(&upserted.results) {
        match &result.status {
            BatchItemStatus::Ok { value } => assert_defaults(value),
            other => panic!("item {}: {other:?}", result.index),
        }
    }
    let reread = items.find_unique("c".to_owned()).run().expect("find");
    assert_defaults(&reread.expect("row c"));
}

/// A table an older version created has no `DEFAULT` clauses, and
/// `CREATE TABLE IF NOT EXISTS` never adds them. The runtime binds the
/// values itself, so creates work on it unchanged.
#[test]
fn a_table_without_default_clauses_still_gets_the_literals() {
    let old = "CREATE TABLE items (id BLOB PRIMARY KEY, done BLOB, count BLOB, big BLOB, \
               ratio BLOB, price BLOB, label BLOB, status BLOB, priority BLOB, ref BLOB, \
               since BLOB, note BLOB, title BLOB)";
    let runtime = runtime(old);
    let items = ModelDelegate::<Item, String>::new(&runtime, &ITEM_MODEL);
    assert_defaults(&items.create(input("a")).run().expect("create"));
}

#[test]
fn create_table_sql_writes_the_same_values_for_raw_inserts() {
    let ddl = create_table_sql(&ITEM_MODEL);
    for clause in [
        "done BLOB DEFAULT 0",
        "count BLOB DEFAULT -3",
        "ratio BLOB DEFAULT 1.5",
        "price BLOB DEFAULT '12.50'",
        r#"label BLOB DEFAULT 'it''s "x"'"#,
        "status BLOB DEFAULT 'Live'",
        "priority BLOB DEFAULT 2",
        "ref BLOB DEFAULT '6f1c2d4e-8b3a-4c5d-9e7f-0a1b2c3d4e5f'",
    ] {
        assert!(ddl.contains(clause), "{clause}: {ddl}");
    }
    let runtime = runtime(&ddl);
    runtime
        .with_connection(|conn| {
            conn.execute("INSERT INTO items (id, title) VALUES ('raw', 'raw')", [])
                .expect("raw insert");
            Ok(())
        })
        .unwrap();
    let items = ModelDelegate::<Item, String>::new(&runtime, &ITEM_MODEL);
    let raw = items.find_unique("raw".to_owned()).run().expect("find");
    assert_defaults(&raw.expect("raw row"));
}
