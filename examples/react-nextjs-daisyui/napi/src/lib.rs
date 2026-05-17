#![allow(non_snake_case)]

//! Server-side N-API addon for the Next.js example.
//!
//! Hosts two CrateStack surfaces:
//!
//! 1. **Embedded SQLite** (`include_embedded_schema!`) — the Note model
//!    persisted to a Node-owned SQLite file. Next.js Route Handlers call
//!    into this for server-side reads/writes and for the offline-first
//!    sync endpoint that the browser pushes deltas to.
//!
//! 2. **Typed HTTP client** (`include_client_schema!`) — calls out to a
//!    remote CrateStack service (the Article contract). Demonstrates the
//!    "trusted Rust on the server, dumb fetch in the browser" pattern
//!    where TLS, cert pinning, and outbound secrets stay in this addon.
//!
//! Both macros emit a `cratestack_schema` module. We wrap each in its own
//! `pub mod notes_schema { ... } / pub mod articles_schema { ... }` so
//! they don't collide; the schema files themselves are resolved relative
//! to this crate's `CARGO_MANIFEST_DIR`, which matches the file layout.

#[cfg(not(target_arch = "wasm32"))]
mod addon {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use chrono::Utc;
    use cratestack_client_rust::{ClientConfig, CratestackClient};
    use cratestack_codec_cbor::CborCodec;
    use cratestack_rusqlite::ddl::create_table_sql;
    use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
    use napi_derive::napi;
    use url::Url;
    use uuid::Uuid;

    pub mod notes_schema {
        use cratestack_macros::include_embedded_schema;
        include_embedded_schema!("notes.cstack");
    }

    pub mod articles_schema {
        use cratestack_macros::include_client_schema;
        include_client_schema!("articles.cstack");
    }

    static RUNTIME: OnceLock<RusqliteRuntime> = OnceLock::new();

    /// JS-facing shape of a Note row. `napi(object)` makes this a plain
    /// JS object (not a class) — matches how Route Handlers want to pass
    /// JSON-shaped data around.
    #[napi(object)]
    pub struct JsNote {
        pub id: String,
        pub title: String,
        pub body: String,
        pub pinned: bool,
        pub completed: bool,
        pub createdAt: String,
        pub updatedAt: String,
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

    #[napi(object)]
    pub struct JsArticle {
        pub id: i64,
        pub title: String,
        pub body: String,
        pub published: bool,
        pub createdAt: String,
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

    fn runtime() -> napi::Result<&'static RusqliteRuntime> {
        RUNTIME.get().ok_or_else(|| {
            napi::Error::from_reason("addon not initialized — call init(dbPath) first")
        })
    }

    /// Open (or create) the SQLite file and bootstrap the `Note` table.
    /// Idempotent: subsequent calls with the same path are no-ops.
    #[napi]
    pub fn init(db_path: String) -> napi::Result<()> {
        if RUNTIME.get().is_some() {
            return Ok(());
        }
        let path = PathBuf::from(&db_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    napi::Error::from_reason(format!("create parent dir: {error}"))
                })?;
            }
        }
        let opened = RusqliteRuntime::open(&path)
            .map_err(|error| napi::Error::from_reason(format!("open failed: {error}")))?;
        opened
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(
                    &notes_schema::cratestack_schema::NOTE_MODEL,
                ))?;
                Ok(())
            })
            .map_err(|error| napi::Error::from_reason(format!("bootstrap failed: {error}")))?;
        // OnceLock::set returns Err only if a parallel init won the race; in
        // that case the other handle is equally valid, so drop ours.
        let _ = RUNTIME.set(opened);
        Ok(())
    }

    #[napi]
    pub fn list_notes() -> napi::Result<Vec<JsNote>> {
        let runtime = runtime()?;
        let notes = ModelDelegate::new(runtime, &notes_schema::cratestack_schema::NOTE_MODEL);
        let rows = notes
            .find_many()
            .order_by(notes_schema::cratestack_schema::note::updatedAt().desc())
            .limit(500)
            .run()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        Ok(rows.into_iter().map(JsNote::from).collect())
    }

    /// Insert or update by id; last-write-wins by `updatedAt`. This is the
    /// server side of the offline-first push.
    #[napi]
    pub fn upsert_note(note: JsNote) -> napi::Result<JsNote> {
        let runtime = runtime()?;
        let model = ModelDelegate::new(runtime, &notes_schema::cratestack_schema::NOTE_MODEL);
        let uuid = Uuid::parse_str(&note.id)
            .map_err(|error| napi::Error::from_reason(format!("bad uuid: {error}")))?;
        let incoming_updated = chrono::DateTime::parse_from_rfc3339(&note.updatedAt)
            .map_err(|error| napi::Error::from_reason(format!("bad updatedAt: {error}")))?
            .with_timezone(&Utc);
        let created = chrono::DateTime::parse_from_rfc3339(&note.createdAt)
            .map_err(|error| napi::Error::from_reason(format!("bad createdAt: {error}")))?
            .with_timezone(&Utc);

        let existing = model
            .find_many()
            .where_(notes_schema::cratestack_schema::note::id().eq(uuid))
            .limit(1)
            .run()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;

        let saved = match existing.into_iter().next() {
            Some(local) if local.updatedAt > incoming_updated => local,
            Some(_) => model
                .update(uuid)
                .set(notes_schema::cratestack_schema::UpdateNoteInput {
                    title: Some(note.title),
                    body: Some(note.body),
                    pinned: Some(note.pinned),
                    completed: Some(note.completed),
                    updatedAt: Some(incoming_updated),
                    ..Default::default()
                })
                .run()
                .map_err(|error| napi::Error::from_reason(error.to_string()))?,
            None => model
                .create(notes_schema::cratestack_schema::CreateNoteInput {
                    id: uuid,
                    title: note.title,
                    body: note.body,
                    pinned: note.pinned,
                    completed: note.completed,
                    createdAt: created,
                    updatedAt: incoming_updated,
                })
                .run()
                .map_err(|error| napi::Error::from_reason(error.to_string()))?,
        };
        Ok(JsNote::from(saved))
    }

    /// Returns rows whose `updatedAt` is strictly newer than the cursor.
    /// Empty cursor returns everything. Pull half of the offline-first sync.
    #[napi]
    pub fn notes_since(cursor: String) -> napi::Result<Vec<JsNote>> {
        let runtime = runtime()?;
        let model = ModelDelegate::new(runtime, &notes_schema::cratestack_schema::NOTE_MODEL);
        let cutoff = if cursor.is_empty() {
            None
        } else {
            Some(
                chrono::DateTime::parse_from_rfc3339(&cursor)
                    .map_err(|error| napi::Error::from_reason(format!("bad cursor: {error}")))?
                    .with_timezone(&Utc),
            )
        };
        let mut query = model
            .find_many()
            .order_by(notes_schema::cratestack_schema::note::updatedAt().asc());
        if let Some(cutoff) = cutoff {
            query = query.where_(notes_schema::cratestack_schema::note::updatedAt().gt(cutoff));
        }
        let rows = query
            .limit(500)
            .run()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        Ok(rows.into_iter().map(JsNote::from).collect())
    }

    /// Calls an upstream CrateStack service for `Article` rows over the
    /// typed client. Use this to demonstrate "Next.js Route Handler ->
    /// trusted Rust HTTP -> upstream service" without exposing the upstream
    /// URL or any cert/secret material to the browser.
    #[napi]
    pub async fn fetch_remote_articles(base_url: String) -> napi::Result<Vec<JsArticle>> {
        let url = Url::parse(&base_url)
            .map_err(|error| napi::Error::from_reason(format!("bad url: {error}")))?;
        let runtime = CratestackClient::new(ClientConfig::new(url), CborCodec);
        let client = articles_schema::cratestack_schema::client::Client::new(runtime);
        let articles_client = client.articles();
        let rows = articles_client
            .list(&[("limit", "20")], &[])
            .await
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        Ok(rows.into_iter().map(JsArticle::from).collect())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use addon::*;
