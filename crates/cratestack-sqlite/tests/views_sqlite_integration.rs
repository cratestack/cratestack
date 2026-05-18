//! Integration test for `view` blocks (ADR-0003) against an
//! in-memory SQLite. Exercises the macro → ViewDelegate path
//! end-to-end on the embedded backend.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack::rusqlite_backend::ViewDelegate;

include_embedded_schema!("tests/fixtures/views_integration.cstack");

use cratestack_schema::models::{ACTIVE_NOTE_VIEW, ActiveNote};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(
                "CREATE TABLE notes (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    archived INTEGER NOT NULL
                );
                 CREATE VIEW active_notes AS
                    SELECT id, title FROM notes WHERE archived = 0;
                 INSERT INTO notes (id, title, archived) VALUES
                    (1, 'first', 0),
                    (2, 'old', 1),
                    (3, 'second', 0);",
            )
            .expect("seed schema + rows");
            Ok(())
        })
        .expect("seed connection");
    runtime
}

#[test]
fn view_find_many_returns_active_rows() {
    let runtime = setup();
    let delegate: ViewDelegate<'_, ActiveNote, i64> =
        ViewDelegate::new(&runtime, &ACTIVE_NOTE_VIEW);

    let mut rows = delegate.find_many().run().expect("find_many returns ok");
    rows.sort_by_key(|row| row.id);

    assert_eq!(rows.len(), 2, "view filters archived notes");
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].title, "first");
    assert_eq!(rows[1].id, 3);
    assert_eq!(rows[1].title, "second");
}

#[test]
fn view_find_unique_returns_single_row() {
    let runtime = setup();
    let delegate: ViewDelegate<'_, ActiveNote, i64> =
        ViewDelegate::new(&runtime, &ACTIVE_NOTE_VIEW);

    let row = delegate
        .find_unique(1)
        .run()
        .expect("find_unique returns ok")
        .expect("active note 1 exists");
    assert_eq!(row.title, "first");

    let archived = delegate
        .find_unique(2)
        .run()
        .expect("find_unique returns ok");
    assert!(
        archived.is_none(),
        "archived note is hidden by the view's SQL body",
    );
}
