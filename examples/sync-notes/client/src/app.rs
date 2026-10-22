//! The device-side app: local note operations, all of them sync-aware.
//!
//! Each write is a *named mutation*: applied to SQLite immediately (so the
//! UI works offline) and queued for the server, which replays the same name
//! through `server/src/sync.rs` under the schema's policies. The local
//! mutators below and those server handlers must agree on what each name
//! means; that pairing is what `@@sync` would generate.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::rusqlite::Connection;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use cratestack_sync::client::{ClientError, DescriptorModel, SyncEngine, SyncStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cratestack_schema::{
    self as schema, CreateNoteInput, NOTE_MODEL, Note, UpdateNoteInput,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
    pub updated_at: DateTime<Utc>,
    /// Carries a local change the server has not acknowledged yet.
    pub unsynced: bool,
}

pub struct NotesApp {
    runtime: Arc<RusqliteRuntime>,
    engine: SyncEngine,
}

#[derive(Deserialize)]
struct WriteArgs {
    id: Uuid,
    title: Option<String>,
    done: Option<bool>,
    at: DateTime<Utc>,
}

fn delegate(runtime: &RusqliteRuntime) -> ModelDelegate<'_, Note, Uuid> {
    ModelDelegate::new(runtime, &NOTE_MODEL)
}

impl NotesApp {
    /// `user` is who this device belongs to; it only fills `ownerId` on
    /// optimistic creates — the server sets the real owner from the
    /// authenticated principal.
    pub fn open(runtime: RusqliteRuntime, user: &str) -> Result<Self, ClientError> {
        let runtime = Arc::new(runtime);
        let owner = user.to_owned();
        let (create_rt, update_rt, delete_rt) = (runtime.clone(), runtime.clone(), runtime.clone());
        let engine = SyncEngine::new()
            .model(DescriptorModel::<Note, Uuid, CreateNoteInput>::new(
                "Note",
                &NOTE_MODEL,
            ))
            .mutation("note.create", move |conn: &Connection, args: &Value| {
                let args: WriteArgs = serde_json::from_value(args.clone())?;
                let input = CreateNoteInput {
                    id: args.id,
                    ownerId: owner.clone(),
                    title: args.title.unwrap_or_default(),
                    done: false,
                    updatedAt: args.at,
                };
                delegate(&create_rt).create(input).run_in_tx(conn)?;
                Ok(())
            })
            .mutation("note.update", move |conn: &Connection, args: &Value| {
                let args: WriteArgs = serde_json::from_value(args.clone())?;
                let patch = UpdateNoteInput {
                    title: args.title,
                    done: args.done,
                    updatedAt: Some(args.at),
                    ..Default::default()
                };
                delegate(&update_rt)
                    .update(args.id)
                    .set(patch)
                    .run_in_tx(conn)?;
                Ok(())
            })
            .mutation("note.delete", move |conn: &Connection, args: &Value| {
                let args: WriteArgs = serde_json::from_value(args.clone())?;
                delegate(&delete_rt).delete(args.id).run_in_tx(conn)?;
                Ok(())
            });
        runtime.with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&NOTE_MODEL))?;
            Ok(())
        })?;
        runtime.with_connection(|conn| engine.install(conn).map_err(to_rusqlite))?;
        Ok(Self { runtime, engine })
    }

    pub fn notes(&self) -> Result<Vec<NoteView>, ClientError> {
        let notes = delegate(&self.runtime)
            .find_many()
            .order_by(schema::note::updatedAt().desc())
            .run()?;
        let unsynced: HashSet<String> = self
            .runtime
            .with_connection(|conn| self.engine.unsynced_keys(conn, "Note").map_err(to_rusqlite))?
            .into_iter()
            .collect();
        Ok(notes
            .into_iter()
            .map(|note| NoteView {
                unsynced: unsynced.contains(&note.id.to_string()),
                id: note.id,
                title: note.title,
                done: note.done,
                updated_at: note.updatedAt,
            })
            .collect())
    }

    pub fn create(&self, title: &str) -> Result<Uuid, ClientError> {
        let id = Uuid::new_v4();
        self.mutate("note.create", json!({ "id": id, "title": title }))?;
        Ok(id)
    }

    pub fn update(
        &self,
        id: Uuid,
        title: Option<&str>,
        done: Option<bool>,
    ) -> Result<(), ClientError> {
        self.mutate(
            "note.update",
            json!({ "id": id, "title": title, "done": done }),
        )
    }

    pub fn delete(&self, id: Uuid) -> Result<(), ClientError> {
        self.mutate("note.delete", json!({ "id": id }))
    }

    /// Timestamps go into the args, so replaying a mutation during a rebase
    /// reproduces it exactly instead of re-reading the clock.
    fn mutate(&self, name: &str, mut args: Value) -> Result<(), ClientError> {
        args["at"] = json!(Utc::now());
        self.runtime
            .with_connection(|conn| self.engine.mutate(conn, name, args).map_err(to_rusqlite))?;
        Ok(())
    }

    pub fn status(&self) -> Result<SyncStatus, ClientError> {
        Ok(self
            .runtime
            .with_connection(|conn| self.engine.status(conn).map_err(to_rusqlite))?)
    }

    /// Run one engine step with exclusive access to the connection.
    pub fn with_engine<T>(
        &self,
        step: impl FnOnce(&SyncEngine, &mut Connection) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        Ok(self
            .runtime
            .with_connection(|conn| step(&self.engine, conn).map_err(to_rusqlite))?)
    }
}

/// `with_connection` speaks `RusqliteError`; carry ours through it as text.
fn to_rusqlite(error: ClientError) -> cratestack_rusqlite::RusqliteError {
    match error {
        ClientError::Rusqlite(inner) => inner,
        ClientError::Sqlite(inner) => inner.into(),
        other => cratestack_rusqlite::RusqliteError::Validation(other.to_string()),
    }
}

#[cfg(test)]
#[path = "tests_app.rs"]
mod tests;
