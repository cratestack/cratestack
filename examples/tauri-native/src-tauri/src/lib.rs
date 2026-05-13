#![allow(non_snake_case)]

//! Tauri 2 shell hosting **both** CrateStack macros natively.
//!
//! Compared to `tauri-web` — which keeps the embedded SQLite path in
//! wasm inside the webview and only the typed HTTP client in native
//! Rust — this example pulls *everything* into the trusted native
//! shell. The renderer is a pure view layer: every data operation,
//! local *and* remote, goes through a `#[tauri::command]`.
//!
//! Both macros emit a `cratestack_schema` module, so we wrap each call
//! in its own `notes_schema { ... }` / `articles_schema { ... }` module
//! and access types through those.

use std::sync::OnceLock;

use chrono::Utc;
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use url::Url;
use uuid::Uuid;

mod notes_schema {
    use cratestack_macros::include_embedded_schema;
    include_embedded_schema!("notes.cstack");
}

mod articles_schema {
    use cratestack_macros::include_client_schema;
    include_client_schema!("articles.cstack");
}

/// Tauri-managed app state. Holds the SQLite runtime so every command
/// reuses the same connection (the runtime serializes access via its
/// own internal `Mutex<Connection>`). The Note `ModelDelegate` is
/// cheap to construct (it's just a pair of references) so each command
/// builds its own rather than threading another generic through here.
struct AppState {
    runtime: RusqliteRuntime,
}

fn note_delegate(
    runtime: &RusqliteRuntime,
) -> ModelDelegate<'_, notes_schema::cratestack_schema::Note, Uuid> {
    ModelDelegate::new(runtime, &notes_schema::cratestack_schema::NOTE_MODEL)
}

/// JS-facing view of a Note row. Plain JSON over the Tauri IPC channel.
#[derive(Debug, Clone, Serialize)]
struct JsNote {
    id: String,
    title: String,
    body: String,
    pinned: bool,
    completed: bool,
    createdAt: String,
    updatedAt: String,
}

impl From<notes_schema::cratestack_schema::Note> for JsNote {
    fn from(value: notes_schema::cratestack_schema::Note) -> Self {
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

/// Shape of `add_note(...)`'s JS-side argument.
#[derive(Debug, Clone, Deserialize)]
struct NewNote {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
struct JsArticle {
    id: i64,
    title: String,
    body: String,
    published: bool,
    createdAt: String,
}

impl From<articles_schema::cratestack_schema::Article> for JsArticle {
    fn from(value: articles_schema::cratestack_schema::Article) -> Self {
        Self {
            id: value.id,
            title: value.title,
            body: value.body,
            published: value.published,
            createdAt: value.createdAt.to_rfc3339(),
        }
    }
}

#[tauri::command]
fn list_notes(state: State<'_, AppState>, only_open: bool) -> Result<Vec<JsNote>, String> {
    let notes = note_delegate(&state.runtime);
    let mut query = notes
        .find_many()
        .order_by(notes_schema::cratestack_schema::note::updatedAt().desc())
        .limit(500);
    if only_open {
        query = query.where_(notes_schema::cratestack_schema::note::completed().is_false());
    }
    query
        .run()
        .map(|rows| rows.into_iter().map(JsNote::from).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_note(state: State<'_, AppState>, input: NewNote) -> Result<JsNote, String> {
    let now = Utc::now();
    note_delegate(&state.runtime)
        .create(notes_schema::cratestack_schema::CreateNoteInput {
            id: Uuid::new_v4(),
            title: input.title,
            body: input.body,
            pinned: input.pinned,
            completed: false,
            createdAt: now,
            updatedAt: now,
        })
        .run()
        .map(JsNote::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn mark_done(state: State<'_, AppState>, id: String) -> Result<JsNote, String> {
    let uuid = Uuid::parse_str(&id).map_err(|error| format!("bad uuid: {error}"))?;
    note_delegate(&state.runtime)
        .update(uuid)
        .set(notes_schema::cratestack_schema::UpdateNoteInput {
            completed: Some(true),
            updatedAt: Some(Utc::now()),
            ..Default::default()
        })
        .run()
        .map(JsNote::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_note(state: State<'_, AppState>, id: String) -> Result<JsNote, String> {
    let uuid = Uuid::parse_str(&id).map_err(|error| format!("bad uuid: {error}"))?;
    note_delegate(&state.runtime)
        .delete(uuid)
        .run()
        .map(JsNote::from)
        .map_err(|error| error.to_string())
}

/// Calls an upstream CrateStack service for `Article` rows over the
/// typed client. Same shape as `tauri-web`'s remote command — the
/// browser never speaks to the upstream directly; cert pinning, TLS,
/// outbound headers, etc. live here.
#[tauri::command]
async fn fetch_remote_articles(base_url: String) -> Result<Vec<JsArticle>, String> {
    let url = Url::parse(&base_url).map_err(|error| format!("bad url: {error}"))?;
    let runtime = CratestackClient::new(ClientConfig::new(url), CborCodec);
    let client = articles_schema::cratestack_schema::client::Client::new(runtime);
    let articles_client = client.articles();
    let rows = articles_client
        .list(&[("limit", "20")], &[])
        .await
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().map(JsArticle::from).collect())
}

#[tauri::command]
fn surface_summary() -> serde_json::Value {
    serde_json::json!({
        "models": notes_schema::cratestack_schema::MODELS,
        "remote_models": articles_schema::cratestack_schema::MODELS,
        "remote_procedures": articles_schema::cratestack_schema::PROCEDURES,
    })
}

/// Allow tests to point the SQLite file at a temp path. Production
/// resolves to `<app_data_dir>/notes.db` (per Tauri's platform-appropriate
/// data dir — `~/Library/Application Support/...` on macOS, `%APPDATA%/...`
/// on Windows, `~/.local/share/...` on Linux).
static DB_PATH_OVERRIDE: OnceLock<std::path::PathBuf> = OnceLock::new();

fn build_app_state(db_path: &std::path::Path) -> Result<AppState, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create app data dir: {error}"))?;
    }
    let runtime = RusqliteRuntime::open(db_path)
        .map_err(|error| format!("open sqlite: {error}"))?;
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&notes_schema::cratestack_schema::NOTE_MODEL))?;
            Ok(())
        })
        .map_err(|error| format!("bootstrap notes table: {error}"))?;
    Ok(AppState { runtime })
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let db_path = if let Some(path) = DB_PATH_OVERRIDE.get() {
                path.clone()
            } else {
                let data_dir = app
                    .path()
                    .app_data_dir()
                    .expect("Tauri must expose an app data dir");
                data_dir.join("notes.db")
            };
            let state = build_app_state(&db_path)
                .expect("AppState must initialize before any command runs");
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_notes,
            add_note,
            mark_done,
            delete_note,
            fetch_remote_articles,
            surface_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// -----------------------------------------------------------------------------
// Native tests: exercise the embedded path against an in-memory SQLite.
// Catches schema / macro regressions without needing the Tauri runtime.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_state() -> AppState {
        let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(
                    &notes_schema::cratestack_schema::NOTE_MODEL,
                ))?;
                Ok(())
            })
            .expect("bootstrap");
        AppState { runtime }
    }

    #[test]
    fn note_crud_round_trip() {
        let state = in_memory_state();
        let notes = note_delegate(&state.runtime);
        let now = Utc::now();
        let created = notes
            .create(notes_schema::cratestack_schema::CreateNoteInput {
                id: Uuid::new_v4(),
                title: "Native!".into(),
                body: "Hello from Tauri".into(),
                pinned: true,
                completed: false,
                createdAt: now,
                updatedAt: now,
            })
            .run()
            .unwrap();
        let view: JsNote = created.into();
        assert_eq!(view.title, "Native!");
        assert!(view.pinned);

        let listed = notes.find_many().run().unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn macro_metadata_surface_is_distinct_per_module() {
        // Sanity check that wrapping each include_*_schema! call in its
        // own module actually isolates the generated symbols — both
        // modules expose a Note-or-Article model name, both are reachable.
        assert!(notes_schema::cratestack_schema::MODELS.contains(&"Note"));
        assert!(articles_schema::cratestack_schema::MODELS.contains(&"Article"));
    }
}
