//! cratestack#1234: on the embedded runtime the create input keeps an
//! `auth()` default as a required field, and a `dbgenerated()` or
//! registered-function default as an `Option`. A required one nothing
//! filled is refused, and the insert rolled back, where it used to be
//! written as NULL and fail decoding.

use cratestack::{RusqliteError, RusqliteRuntime, include_embedded_schema};
use cratestack_core::BatchItemStatus;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};
use uuid::Uuid;

include_embedded_schema!("tests/fixtures/database_defaults.cstack");

use cratestack_schema::models::Entry;
use cratestack_schema::{CreateEntryInput, ENTRY_MODEL};

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

fn input(seq: Option<i64>, code: Option<&str>) -> CreateEntryInput {
    CreateEntryInput {
        owner: "user-1".to_owned(),
        seq,
        code: code.map(str::to_owned),
        note: None,
        title: "t".to_owned(),
    }
}

fn count(runtime: &RusqliteRuntime) -> i64 {
    runtime
        .with_connection(|conn| {
            Ok(conn.query_row("SELECT count(*) FROM entries", [], |row| row.get(0))?)
        })
        .unwrap()
}

#[test]
fn values_the_input_sets_are_bound() {
    let runtime = runtime(&create_table_sql(&ENTRY_MODEL));
    let entries = ModelDelegate::<Entry, Uuid>::new(&runtime, &ENTRY_MODEL);
    let entry = entries
        .create(input(Some(7), Some("A-1")))
        .run()
        .expect("create");
    assert_eq!(
        (entry.owner.as_str(), entry.seq, entry.code.as_str()),
        ("user-1", 7, "A-1")
    );
    assert_eq!(entry.note, None);
}

#[test]
fn an_unset_column_nothing_fills_is_refused_and_rolled_back() {
    let runtime = runtime(&create_table_sql(&ENTRY_MODEL));
    let entries = ModelDelegate::<Entry, Uuid>::new(&runtime, &ENTRY_MODEL);

    let error = entries
        .create(input(None, Some("A-1")))
        .run()
        .expect_err("nothing fills seq");
    match &error {
        RusqliteError::UnfilledDefault(unfilled) => {
            assert_eq!(
                (unfilled.column, unfilled.default),
                ("seq", "dbgenerated()")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        error.to_string().contains("trigger or a DEFAULT"),
        "{error}"
    );
    assert_eq!(count(&runtime), 0, "the insert is rolled back");

    let batch = entries
        .batch_create(vec![input(Some(1), None), input(Some(2), Some("B"))])
        .run()
        .expect("batch");
    match &batch.results[0].status {
        BatchItemStatus::Error { error } => {
            assert_eq!(error.code, "VALIDATION_ERROR");
            assert!(error.message.contains("next_label()"), "{}", error.message);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        batch.results[1].status,
        BatchItemStatus::Ok { .. }
    ));
    assert_eq!(count(&runtime), 1);
}

/// The app's own `DEFAULT`, or a trigger, fills an unset column.
#[test]
fn a_database_default_the_app_adds_fills_an_unset_column() {
    let ddl = "CREATE TABLE entries (id BLOB PRIMARY KEY, owner BLOB, seq BLOB DEFAULT 42, \
               code BLOB DEFAULT ('C-' || 1), note BLOB, title BLOB)";
    let runtime = runtime(ddl);
    let entries = ModelDelegate::<Entry, Uuid>::new(&runtime, &ENTRY_MODEL);
    let entry = entries.create(input(None, None)).run().expect("create");
    assert_eq!((entry.seq, entry.code.as_str()), (42, "C-1"));
}
