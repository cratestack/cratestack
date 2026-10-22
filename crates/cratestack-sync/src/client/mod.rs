//! Client half: a sans-IO sync engine over `rusqlite`.
//!
//! The engine never touches the network. The host (a wasm worker, a
//! Flutter isolate, a tokio task) asks it for a request, sends it however
//! it likes, and hands the response back:
//!
//! ```text
//! engine.mutate(conn, "note.create", args)?;          // local, instant, offline-safe
//! if let Some(req) = engine.push_request(conn)? { let res = http(req); engine.apply_push(conn, res)?; }
//! loop { let res = http(engine.pull_request(conn)?); if !engine.apply_pull(conn, res)?.has_more { break } }
//! on a SyncErrorBody from any of them: engine.recover(conn, &error)?;
//! ```
//!
//! Every method takes `&mut Connection` and does its work in one SQLite
//! transaction, so the UI never observes a half-applied pull.

mod ddl;
mod flow;
mod inspect;
mod model;
mod rebase;
mod recovery;
mod store;

#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod tests_rebase;
#[cfg(test)]
mod tests_support;

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use cratestack_rusqlite::RusqliteError;
use cratestack_rusqlite::rusqlite::{self, Connection, params};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub use flow::{PullOutcome, PushOutcome, Recovery};
pub use model::{DescriptorModel, LocalMutator, SyncedModel};
pub use store::RejectedMutation;

use crate::protocol::Cursor;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Rusqlite(#[from] RusqliteError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("no local mutator named `{0}`")]
    UnknownMutation(String),
    #[error("{0} mutations are already queued offline; sync before writing more")]
    QueueFull(u64),
    /// A mutator refused its args (validation in the app's own code).
    #[error("{0}")]
    Mutator(String),
    #[error("protocol: {0}")]
    Protocol(String),
}

/// What the UI needs to render sync state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub client_id: Option<Uuid>,
    pub cursor: Cursor,
    /// Not yet acknowledged by the server.
    pub pending: u64,
    /// Not yet sent to the server.
    pub unpushed: u64,
    pub rejected: Vec<RejectedMutation>,
}

#[derive(Clone)]
pub struct SyncEngine {
    models: Vec<Arc<dyn SyncedModel>>,
    mutators: HashMap<String, Arc<dyn LocalMutator>>,
    max_pending: u64,
}

/// Not derived: a derived `Default` would set `max_pending` to 0 and make
/// every `mutate` fail with `QueueFull(0)`.
impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            mutators: HashMap::new(),
            max_pending: crate::SyncPolicy::default().max_pending_mutations,
        }
    }

    pub fn model(mut self, model: impl SyncedModel + 'static) -> Self {
        self.models.push(Arc::new(model));
        self
    }

    pub fn mutation(
        mut self,
        name: impl Into<String>,
        mutator: impl LocalMutator + 'static,
    ) -> Self {
        self.mutators.insert(name.into(), Arc::new(mutator));
        self
    }

    pub fn max_pending(mut self, max: u64) -> Self {
        self.max_pending = max;
        self
    }

    /// Create bookkeeping tables, shadows and capture triggers. Run after
    /// the app's own tables exist. Idempotent.
    pub fn install(&self, conn: &mut Connection) -> Result<(), ClientError> {
        let tx = conn.transaction()?;
        tx.execute_batch(ddl::CLIENT_DDL)?;
        for model in &self.models {
            let shadow = ddl::shadow_table(model.table());
            let shadow_rows: Option<i64> = tx
                .query_row(&format!("SELECT count(*) FROM {shadow}"), [], |r| r.get(0))
                .ok();
            if shadow_rows == Some(0) {
                // Empty: rebuild from the current columns (app upgrades).
                tx.execute_batch(&format!("DROP TABLE {shadow}"))?;
            }
            tx.execute_batch(&ddl::model_ddl(
                model.model(),
                model.table(),
                model.pk_column(),
            ))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply a mutation locally and queue it for the server, atomically.
    pub fn mutate(
        &self,
        conn: &mut Connection,
        name: &str,
        args: Value,
    ) -> Result<u64, ClientError> {
        let mutator = self
            .mutators
            .get(name)
            .ok_or_else(|| ClientError::UnknownMutation(name.to_owned()))?;
        let tx = conn.transaction()?;
        let queued: i64 =
            tx.query_row("SELECT count(*) FROM cratestack_sync_pending", [], |r| {
                r.get(0)
            })?;
        if queued as u64 >= self.max_pending {
            return Err(ClientError::QueueFull(queued as u64));
        }
        let seq = store::get_u64(&tx, store::NEXT_SEQ)?.max(1);
        rebase::capturing(&tx, || mutator.apply(&tx, &args))?;
        tx.execute(
            "INSERT INTO cratestack_sync_pending (seq, name, args, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![seq as i64, name, serde_json::to_string(&args)?, Utc::now().to_rfc3339()],
        )?;
        store::set(&tx, store::NEXT_SEQ, seq + 1)?;
        tx.commit()?;
        Ok(seq)
    }

    fn find_model(&self, name: &str) -> Option<&Arc<dyn SyncedModel>> {
        self.models.iter().find(|m| m.model() == name)
    }
}
