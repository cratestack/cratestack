#![allow(non_snake_case)]

//! React + Vite + DaisyUI browser example.
//!
//! Same `include_embedded_schema!` + `ModelDelegate` shape as
//! `embedded-browser-vite` — the difference here is purely on the JS side:
//! the renderer is React 19 + Tailwind 4 + DaisyUI 5 instead of vanilla TS.
//! The Rust surface is identical, which is the point: CrateStack's embedded
//! shape doesn't care which UI framework wraps it.

use chrono::Utc;
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

#[derive(Debug, Clone, Deserialize)]
pub struct NewNote {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
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
        let created = notes
            .create(cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "First note".into(),
                body: "Hello from React + Vite".into(),
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
        let listed = notes.find_many().run().unwrap();
        assert_eq!(listed.len(), 1);
    }
}
