// `.cstack` schemas use camelCase field names by convention; the generated
// model struct mirrors that, and `NoteView` shadows it for the JS side. Allow
// the case mismatch crate-wide rather than renaming through the wire.
#![allow(non_snake_case)]

//! Browser-targeted embedded SQLite example.
//!
//! Same `.cstack` schema and same `include_embedded_schema!`-driven
//! `ModelDelegate` API as the native `embedded-cli` example — only the
//! runtime open path and the JS-facing surface differ. The Rust code below
//! compiles to `wasm32-unknown-unknown`; the JS side (in `web/`) loads the
//! resulting wasm bundle inside a Dedicated Worker, installs OPFS for
//! persistence, and calls these exports over a `postMessage` RPC.
//!
//! The `#[cfg(target_arch = "wasm32")]` gates around the wasm-bindgen
//! surface let the same crate `cargo check` cleanly on native (without
//! pulling wasm-bindgen) so the workspace stays green on every platform.

use chrono::Utc;
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

/// Shape of `add_note(...)`'s JS-side argument.
#[derive(Debug, Clone, Deserialize)]
pub struct NewNote {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

/// Shape of the JS-side view of a `Note` row. Mirrors the generated model
/// struct but uses string `id` (uuid-as-text) for JSON-friendly transport.
#[derive(Debug, Clone, Serialize)]
pub struct NoteView {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub completed: bool,
    pub createdAt: String,
    pub updatedAt: String,
}

impl From<cratestack_schema::Note> for NoteView {
    fn from(value: cratestack_schema::Note) -> Self {
        Self {
            id: value.id.hyphenated().to_string(),
            title: value.title,
            body: value.body,
            pinned: value.pinned,
            completed: value.completed,
            createdAt: value.createdAt.to_rfc3339(),
            updatedAt: value.updatedAt.to_rfc3339(),
        }
    }
}

// -----------------------------------------------------------------------------
// wasm-bindgen surface
// -----------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use cratestack_rusqlite::opfs;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static RUNTIME: RefCell<Option<RusqliteRuntime>> = RefCell::new(None);
    }

    /// Install the panic hook so Rust panics produce useful browser
    /// console output. Call once on worker startup.
    #[wasm_bindgen]
    pub fn init_panic_hook() {
        console_error_panic_hook::set_once();
    }

    /// Install the OPFS SAH-pool VFS. **Must be called inside a Dedicated
    /// Worker** — OPFS `SyncAccessHandle` is worker-only by spec.
    #[wasm_bindgen]
    pub async fn install_opfs() -> Result<(), JsValue> {
        opfs::install_opfs_vfs(&opfs::OpfsOptions::default())
            .await
            .map_err(|error| JsValue::from_str(&format!("opfs install failed: {error}")))
    }

    /// Open (or create) the database file. Pass a name like `"notes.db"`;
    /// the SAH-pool VFS persists it under its OPFS metadata directory.
    /// Bootstraps the `Note` table on first call.
    #[wasm_bindgen]
    pub fn open_db(filename: &str) -> Result<(), JsValue> {
        let runtime = RusqliteRuntime::open(filename)
            .map_err(|error| JsValue::from_str(&format!("open failed: {error}")))?;
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .map_err(|error| JsValue::from_str(&format!("bootstrap failed: {error}")))?;
        RUNTIME.with(|cell| {
            *cell.borrow_mut() = Some(runtime);
        });
        Ok(())
    }

    /// Open an in-memory database. Useful when OPFS is unavailable (e.g.
    /// when the host page isn't running this code inside a worker).
    #[wasm_bindgen]
    pub fn open_in_memory() -> Result<(), JsValue> {
        let runtime = RusqliteRuntime::open_in_memory()
            .map_err(|error| JsValue::from_str(&format!("open_in_memory failed: {error}")))?;
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .map_err(|error| JsValue::from_str(&format!("bootstrap failed: {error}")))?;
        RUNTIME.with(|cell| {
            *cell.borrow_mut() = Some(runtime);
        });
        Ok(())
    }

    fn with_runtime<R>(
        f: impl FnOnce(&RusqliteRuntime) -> Result<R, cratestack_rusqlite::RusqliteError>,
    ) -> Result<R, JsValue> {
        RUNTIME.with(|cell| match cell.borrow().as_ref() {
            Some(runtime) => f(runtime).map_err(|error| JsValue::from_str(&error.to_string())),
            None => Err(JsValue::from_str(
                "database not open — call open_db / open_in_memory first",
            )),
        })
    }

    #[wasm_bindgen]
    pub fn add_note(input: JsValue) -> Result<JsValue, JsValue> {
        let parsed: NewNote = serde_wasm_bindgen::from_value(input)
            .map_err(|error| JsValue::from_str(&format!("invalid input: {error}")))?;
        let now = Utc::now();
        let id = Uuid::new_v4();
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            notes
                .create(cratestack_schema::CreateNoteInput {
                    id,
                    title: parsed.title,
                    body: parsed.body,
                    pinned: parsed.pinned,
                    completed: false,
                    createdAt: now,
                    updatedAt: now,
                })
                .run()
        })
        .and_then(|note| {
            serde_wasm_bindgen::to_value(&NoteView::from(note))
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }

    #[wasm_bindgen]
    pub fn list_notes(only_open: bool) -> Result<JsValue, JsValue> {
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            let mut query = notes
                .find_many()
                .order_by(cratestack_schema::note::createdAt().desc())
                .limit(200);
            if only_open {
                query = query.where_(cratestack_schema::note::completed().is_false());
            }
            query.run()
        })
        .and_then(|rows| {
            let views: Vec<NoteView> = rows.into_iter().map(NoteView::from).collect();
            serde_wasm_bindgen::to_value(&views)
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }

    #[wasm_bindgen]
    pub fn mark_done(id: &str) -> Result<JsValue, JsValue> {
        let uuid =
            Uuid::parse_str(id).map_err(|error| JsValue::from_str(&format!("bad uuid: {error}")))?;
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            notes
                .update(uuid)
                .set(cratestack_schema::UpdateNoteInput {
                    completed: Some(true),
                    updatedAt: Some(Utc::now()),
                    ..Default::default()
                })
                .run()
        })
        .and_then(|note| {
            serde_wasm_bindgen::to_value(&NoteView::from(note))
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }

    #[wasm_bindgen]
    pub fn delete_note(id: &str) -> Result<JsValue, JsValue> {
        let uuid =
            Uuid::parse_str(id).map_err(|error| JsValue::from_str(&format!("bad uuid: {error}")))?;
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            notes.delete(uuid).run()
        })
        .and_then(|note| {
            serde_wasm_bindgen::to_value(&NoteView::from(note))
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }
}

// Re-export the wasm exports at crate root so wasm-bindgen sees them.
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

// -----------------------------------------------------------------------------
// Native test: same delegate paths exercised in-memory. Catches schema /
// macro regressions without needing a browser.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_crud_round_trip() {
        let runtime = RusqliteRuntime::open_in_memory().unwrap();
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .unwrap();
        let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
        let now = Utc::now();
        let created = notes
            .create(cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "First note".into(),
                body: "Hello from wasm".into(),
                pinned: true,
                completed: false,
                createdAt: now,
                updatedAt: now,
            })
            .run()
            .unwrap();
        let view: NoteView = created.into();
        assert_eq!(view.title, "First note");
        assert!(view.pinned);
        assert!(!view.completed);

        let listed = notes.find_many().run().unwrap();
        assert_eq!(listed.len(), 1);
    }
}
