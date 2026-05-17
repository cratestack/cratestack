#![allow(non_snake_case)]

//! Browser side of the Next.js example.
//!
//! Same shape as `react-vite-daisyui`'s wasm crate plus one extra entry
//! point: `upsert_note` (idempotent insert-or-update by id, used when the
//! sync layer pulls server-authored rows into the local OPFS cache).
//!
//! All sync state — `pending` rows waiting to push, `serverUpdatedAt`,
//! `dirty` flag — is tracked client-side in the wasm-bindgen wrappers
//! below, on top of the same generated `ModelDelegate`. The schema itself
//! stays clean of "client bookkeeping" concerns.

use cratestack_macros::include_embedded_schema;
use serde::{Deserialize, Serialize};

include_embedded_schema!("schema.cstack");

#[derive(Debug, Clone, Deserialize)]
pub struct NewNote {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

/// Server-authored note pulled down by the sync layer. Mirrors what the
/// napi-rs addon emits over the Next.js route handler.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteNote {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub completed: bool,
    pub createdAt: String,
    pub updatedAt: String,
}

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

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use cratestack_rusqlite::opfs;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static RUNTIME: RefCell<Option<RusqliteRuntime>> = RefCell::new(None);
    }

    #[wasm_bindgen]
    pub fn init_panic_hook() {
        console_error_panic_hook::set_once();
    }

    #[wasm_bindgen]
    pub async fn install_opfs() -> Result<(), JsValue> {
        opfs::install_opfs_vfs(&opfs::OpfsOptions::default())
            .await
            .map_err(|error| JsValue::from_str(&format!("opfs install failed: {error}")))
    }

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
                .limit(500);
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
        let uuid = Uuid::parse_str(id)
            .map_err(|error| JsValue::from_str(&format!("bad uuid: {error}")))?;
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
        let uuid = Uuid::parse_str(id)
            .map_err(|error| JsValue::from_str(&format!("bad uuid: {error}")))?;
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            notes.delete(uuid).run()
        })
        .and_then(|note| {
            serde_wasm_bindgen::to_value(&NoteView::from(note))
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }

    /// Idempotent merge of a server-authored row into the local OPFS cache.
    /// Used by the pull half of the sync layer: for each row the server
    /// returns, we either insert it or rewrite the local copy to match.
    ///
    /// Last-write-wins by `updatedAt`. If our local row is *newer* than the
    /// server row, we keep ours — the next push will reconcile.
    #[wasm_bindgen]
    pub fn upsert_remote_note(input: JsValue) -> Result<JsValue, JsValue> {
        let remote: RemoteNote = serde_wasm_bindgen::from_value(input)
            .map_err(|error| JsValue::from_str(&format!("invalid input: {error}")))?;
        let uuid = Uuid::parse_str(&remote.id)
            .map_err(|error| JsValue::from_str(&format!("bad uuid: {error}")))?;
        let remote_updated = chrono::DateTime::parse_from_rfc3339(&remote.updatedAt)
            .map_err(|error| JsValue::from_str(&format!("bad updatedAt: {error}")))?
            .with_timezone(&chrono::Utc);
        let created = chrono::DateTime::parse_from_rfc3339(&remote.createdAt)
            .map_err(|error| JsValue::from_str(&format!("bad createdAt: {error}")))?
            .with_timezone(&chrono::Utc);

        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            let existing = notes
                .find_many()
                .where_(cratestack_schema::note::id().eq(uuid))
                .limit(1)
                .run()?;
            match existing.into_iter().next() {
                Some(local) if local.updatedAt > remote_updated => {
                    // Local is newer; the next push will reconcile.
                    Ok(local)
                }
                Some(_) => notes
                    .update(uuid)
                    .set(cratestack_schema::UpdateNoteInput {
                        title: Some(remote.title.clone()),
                        body: Some(remote.body.clone()),
                        pinned: Some(remote.pinned),
                        completed: Some(remote.completed),
                        updatedAt: Some(remote_updated),
                        ..Default::default()
                    })
                    .run(),
                None => notes
                    .create(cratestack_schema::CreateNoteInput {
                        id: uuid,
                        title: remote.title,
                        body: remote.body,
                        pinned: remote.pinned,
                        completed: remote.completed,
                        createdAt: created,
                        updatedAt: remote_updated,
                    })
                    .run(),
            }
        })
        .and_then(|note| {
            serde_wasm_bindgen::to_value(&NoteView::from(note))
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }

    /// Returns rows whose `updatedAt` is strictly newer than the cursor (an
    /// RFC3339 timestamp) or all rows if the cursor is empty. The sync layer
    /// passes the cursor it received last time the server confirmed
    /// receipt, so we only push deltas.
    #[wasm_bindgen]
    pub fn notes_since(cursor: &str) -> Result<JsValue, JsValue> {
        let cutoff = if cursor.is_empty() {
            None
        } else {
            Some(
                chrono::DateTime::parse_from_rfc3339(cursor)
                    .map_err(|error| JsValue::from_str(&format!("bad cursor: {error}")))?
                    .with_timezone(&chrono::Utc),
            )
        };
        with_runtime(|runtime| {
            let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);
            let mut query = notes
                .find_many()
                .order_by(cratestack_schema::note::updatedAt().asc());
            if let Some(cutoff) = cutoff {
                query = query.where_(cratestack_schema::note::updatedAt().gt(cutoff));
            }
            query.run()
        })
        .and_then(|rows| {
            let views: Vec<NoteView> = rows.into_iter().map(NoteView::from).collect();
            serde_wasm_bindgen::to_value(&views)
                .map_err(|error| JsValue::from_str(&format!("serialize: {error}")))
        })
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

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
        notes
            .create(cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "Hello from Next.js".into(),
                body: "wasm side".into(),
                pinned: false,
                completed: false,
                createdAt: now,
                updatedAt: now,
            })
            .run()
            .unwrap();
        let listed = notes.find_many().run().unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn delta_filter_respects_cursor() {
        let runtime = RusqliteRuntime::open_in_memory().unwrap();
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .unwrap();
        let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
        let earlier = chrono::Utc::now() - chrono::Duration::seconds(60);
        let later = chrono::Utc::now();
        let _old = notes
            .create(cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "old".into(),
                body: String::new(),
                pinned: false,
                completed: false,
                createdAt: earlier,
                updatedAt: earlier,
            })
            .run()
            .unwrap();
        let _new = notes
            .create(cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "new".into(),
                body: String::new(),
                pinned: false,
                completed: false,
                createdAt: later,
                updatedAt: later,
            })
            .run()
            .unwrap();
        let cutoff = earlier + chrono::Duration::seconds(1);
        let recent = notes
            .find_many()
            .where_(cratestack_schema::note::updatedAt().gt(cutoff))
            .run()
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].title, "new");
    }
}
