//! Minimal SQLite-on-device quickstart.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example sqlite_quickstart -p cratestack
//! ```
//!
//! Demonstrates the smallest useful CrateStack-on-SQLite program:
//! open an in-memory database, bootstrap the schema, insert a row,
//! read it back. Everything the on-device runtime needs is here.

use cratestack::include_schema;
use cratestack::{RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};

include_schema!("examples/sqlite_quickstart.cstack");

use cratestack_rusqlite::ModelDelegate;
use cratestack_schema::models::Note;
use cratestack_schema::{CreateNoteInput, NOTE_MODEL};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open the database. On a real mobile build you'd use
    //    `RusqliteRuntime::open("path/to/app.db")` against a writable
    //    directory the platform provides.
    let runtime = RusqliteRuntime::open_in_memory()?;

    // 2. Bootstrap the schema. The DDL generator walks the macro-emitted
    //    descriptor and produces a `CREATE TABLE IF NOT EXISTS` statement.
    //    Apps typically run this once per app start.
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&NOTE_MODEL))
            .expect("create table");
        Ok(())
    })?;

    // 3. Build the per-model delegate. Cheap to construct, do it
    //    on-demand at the call site.
    let notes = ModelDelegate::<Note, uuid::Uuid>::new(&runtime, &NOTE_MODEL);

    // 4. Insert a row.
    let id = uuid::Uuid::new_v4();
    let created = notes
        .create(CreateNoteInput {
            id,
            title: "First note".to_string(),
            body: "Hello from a Rust-only frontend.".to_string(),
            pinned: true,
            createdAt: chrono::Utc::now(),
        })
        .run()?;
    println!("inserted note {} (pinned={})", created.id, created.pinned);

    // 5. Read it back through the typed API.
    let fetched = notes
        .find_unique(id)
        .run()?
        .expect("note we just inserted exists");
    println!("fetched: {} — {}", fetched.title, fetched.body);

    // 6. List all notes ordered by creation time, newest first.
    let all = notes
        .find_many()
        .order_by(cratestack_schema::note::createdAt().desc())
        .run()?;
    println!("total notes in store: {}", all.len());

    Ok(())
}
