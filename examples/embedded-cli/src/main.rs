//! Desktop note-taking CLI on top of `cratestack-rusqlite`.
//!
//! Demonstrates the file-backed-SQLite use case (laptops, servers, internal
//! tools). The same `include_embedded_schema!`-generated code compiles
//! unchanged for mobile FFI bridges and `wasm32-unknown-unknown` browser
//! targets — only the runtime open path differs.

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use clap::{Parser, Subcommand};
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

#[derive(Parser)]
#[command(
    name = "notes",
    about = "Embedded SQLite note CLI — drives cratestack-rusqlite",
    version
)]
struct Cli {
    /// Database file. Defaults to ./notes.db in the current directory.
    #[arg(long, default_value = "notes.db", global = true)]
    db: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new note.
    Add {
        /// Note title.
        title: String,
        /// Body text.
        #[arg(long, default_value = "")]
        body: String,
        /// Pin this note to the top of the list.
        #[arg(long)]
        pinned: bool,
    },

    /// List notes, newest first. `--pinned` shows only pinned, `--open`
    /// hides completed.
    List {
        #[arg(long)]
        pinned: bool,
        #[arg(long)]
        open: bool,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },

    /// Mark a note as completed.
    Done {
        /// Note id (uuid).
        id: Uuid,
    },

    /// Delete a note by id.
    Delete {
        id: Uuid,
    },

    /// Print the row count (lightweight smoke test).
    Count,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let runtime = match RusqliteRuntime::open(&cli.db) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to open {}: {error}", cli.db.display());
            return ExitCode::from(2);
        }
    };
    if let Err(error) = bootstrap(&runtime) {
        eprintln!("failed to bootstrap schema: {error}");
        return ExitCode::from(2);
    }

    let result = match cli.command {
        Command::Add {
            title,
            body,
            pinned,
        } => add(&runtime, title, body, pinned),
        Command::List {
            pinned,
            open,
            limit,
        } => list(&runtime, pinned, open, limit),
        Command::Done { id } => done(&runtime, id),
        Command::Delete { id } => delete(&runtime, id),
        Command::Count => count(&runtime),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn bootstrap(runtime: &RusqliteRuntime) -> Result<(), cratestack_rusqlite::RusqliteError> {
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
        Ok(())
    })
}

fn add(
    runtime: &RusqliteRuntime,
    title: String,
    body: String,
    pinned: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
    let now = Utc::now();
    let created = notes
        .create(cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title,
            body,
            pinned,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()?;
    println!("{}  {}", created.id, created.title);
    Ok(())
}

fn list(
    runtime: &RusqliteRuntime,
    only_pinned: bool,
    only_open: bool,
    limit: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
    let mut query = notes
        .find_many()
        .order_by(cratestack_schema::note::createdAt().desc())
        .limit(limit);

    if only_pinned {
        query = query.where_(cratestack_schema::note::pinned().is_true());
    }
    if only_open {
        query = query.where_(cratestack_schema::note::completed().is_false());
    }

    let rows = query.run()?;
    if rows.is_empty() {
        println!("(no notes match)");
        return Ok(());
    }

    for note in rows {
        let marker = match (note.pinned, note.completed) {
            (true, true) => "📌✓",
            (true, false) => "📌 ",
            (false, true) => "  ✓",
            (false, false) => "   ",
        };
        println!("{marker}  {}  {}", note.id, note.title);
        if !note.body.is_empty() {
            for line in note.body.lines() {
                println!("       {line}");
            }
        }
    }
    Ok(())
}

fn done(runtime: &RusqliteRuntime, id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
    let updated = notes
        .update(id)
        .set(cratestack_schema::UpdateNoteInput {
            completed: Some(true),
            updatedAt: Some(Utc::now()),
            ..Default::default()
        })
        .run()?;
    println!("done: {} — {}", updated.id, updated.title);
    Ok(())
}

fn delete(runtime: &RusqliteRuntime, id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
    let removed = notes.delete(id).run()?;
    println!("deleted: {} — {}", removed.id, removed.title);
    Ok(())
}

fn count(runtime: &RusqliteRuntime) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
    let rows = notes.find_many().run()?;
    println!("{} notes", rows.len());
    Ok(())
}
