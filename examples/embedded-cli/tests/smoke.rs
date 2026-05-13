//! Library-style smoke test that exercises the same schema + delegate calls
//! the CLI uses, without spawning a subprocess. Drives `cargo test` from CI.

use chrono::Utc;
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

fn open() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("in-memory db should open");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
            Ok(())
        })
        .expect("DDL should run");
    runtime
}

#[test]
fn create_then_find() {
    let runtime = open();
    let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
    let now = Utc::now();
    let id = Uuid::new_v4();
    let created = notes
        .create(cratestack_schema::CreateNoteInput {
            id,
            title: "Test".into(),
            body: "Hello".into(),
            pinned: true,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .expect("create should succeed");
    assert_eq!(created.title, "Test");
    assert!(created.pinned);
    assert!(!created.completed);

    let one = notes
        .find_unique(id)
        .run()
        .expect("find_unique should succeed");
    assert!(one.is_some());
}

#[test]
fn filter_pinned_and_open() {
    let runtime = open();
    let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
    let now = Utc::now();

    notes
        .create(cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title: "A".into(),
            body: String::new(),
            pinned: true,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .unwrap();
    notes
        .create(cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title: "B".into(),
            body: String::new(),
            pinned: false,
            completed: true,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .unwrap();
    notes
        .create(cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title: "C".into(),
            body: String::new(),
            pinned: false,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .unwrap();

    let pinned = notes
        .find_many()
        .where_(cratestack_schema::note::pinned().is_true())
        .run()
        .unwrap();
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned[0].title, "A");

    let open = notes
        .find_many()
        .where_(cratestack_schema::note::completed().is_false())
        .run()
        .unwrap();
    assert_eq!(open.len(), 2);
}
