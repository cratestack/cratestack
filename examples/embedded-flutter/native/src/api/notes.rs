//! The Dart-callable API surface.
//!
//! flutter_rust_bridge converts each `pub fn` here into a Dart function
//! under `lib/src/rust/api/notes.dart` after you run
//! `flutter_rust_bridge_codegen generate`. The function signatures are
//! the wire contract — types like `String`, `Vec<T>`, primitives, and
//! plain `#[derive(Clone)]` structs travel cleanly; types with
//! generics or trait bounds (UUID, DateTime<Utc>) need to be flattened
//! to strings or ints on this boundary.
//!
//! State management: we keep a single `RusqliteRuntime` behind a
//! `OnceLock`. Dart calls `init_database(path)` once at app startup,
//! then any subsequent call uses the runtime stored here. The runtime
//! serializes its own connection via an internal `Mutex`, so all six
//! of the API calls below can be invoked freely from any Dart isolate.

use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::Utc;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteError, RusqliteRuntime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema;

static RUNTIME: OnceLock<RusqliteRuntime> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteView {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub completed: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewNote {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

/// flutter_rust_bridge generates `pub fn note_to_view(...)` as a
/// non-Dart Rust helper because it takes a non-bridgeable input. It
/// stays as a normal `pub fn` in this module so the native tests can
/// reach it from `tests::*`.
pub fn note_to_view(value: schema::cratestack_schema::Note) -> NoteView {
    NoteView {
        id: value.id.hyphenated().to_string(),
        title: value.title,
        body: value.body,
        pinned: value.pinned,
        completed: value.completed,
        created_at: value.createdAt.to_rfc3339(),
        updated_at: value.updatedAt.to_rfc3339(),
    }
}

fn map_err(error: RusqliteError) -> String {
    error.to_string()
}

fn runtime() -> Result<&'static RusqliteRuntime, String> {
    RUNTIME
        .get()
        .ok_or_else(|| "database not initialized — call init_database() first".to_owned())
}

fn note_delegate(
    runtime: &RusqliteRuntime,
) -> ModelDelegate<'_, schema::cratestack_schema::Note, Uuid> {
    ModelDelegate::new(runtime, &schema::cratestack_schema::NOTE_MODEL)
}

/// Open (or create) the SQLite file at `db_path` and bootstrap the
/// `Note` table. Idempotent — second/Nth calls with the same handle
/// are no-ops because of the `OnceLock`.
pub fn init_database(db_path: String) -> Result<(), String> {
    if RUNTIME.get().is_some() {
        return Ok(());
    }
    let path = PathBuf::from(db_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    let opened = RusqliteRuntime::open(&path).map_err(map_err)?;
    opened
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&schema::cratestack_schema::NOTE_MODEL))?;
            Ok(())
        })
        .map_err(map_err)?;
    // Ignore the Err — it just means a concurrent caller won the race.
    let _ = RUNTIME.set(opened);
    Ok(())
}

pub fn add_note(input: NewNote) -> Result<NoteView, String> {
    let runtime = runtime()?;
    let now = Utc::now();
    note_delegate(runtime)
        .create(schema::cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title: input.title,
            body: input.body,
            pinned: input.pinned,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .map(note_to_view)
        .map_err(map_err)
}

pub fn list_notes(only_open: bool) -> Result<Vec<NoteView>, String> {
    let runtime = runtime()?;
    let mut query = note_delegate(runtime)
        .find_many()
        .order_by(schema::cratestack_schema::note::updatedAt().desc())
        .limit(500);
    if only_open {
        query = query.where_(schema::cratestack_schema::note::completed().is_false());
    }
    query
        .run()
        .map(|rows| rows.into_iter().map(note_to_view).collect())
        .map_err(map_err)
}

pub fn mark_done(id: String) -> Result<NoteView, String> {
    let runtime = runtime()?;
    let uuid = Uuid::parse_str(&id).map_err(|error| format!("bad uuid: {error}"))?;
    note_delegate(runtime)
        .update(uuid)
        .set(schema::cratestack_schema::UpdateNoteInput {
            completed: Some(true),
            updatedAt: Some(Utc::now()),
            ..Default::default()
        })
        .run()
        .map(note_to_view)
        .map_err(map_err)
}

pub fn delete_note(id: String) -> Result<NoteView, String> {
    let runtime = runtime()?;
    let uuid = Uuid::parse_str(&id).map_err(|error| format!("bad uuid: {error}"))?;
    note_delegate(runtime)
        .delete(uuid)
        .run()
        .map(note_to_view)
        .map_err(map_err)
}
