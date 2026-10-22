//! The two application-supplied registries: how to read a synced model's
//! rows for a puller, and how to apply a named mutation for a pusher.
//!
//! Both are hand-written in the prototype. The `@@sync` schema surface the
//! design doc proposes would generate them from the model's descriptor —
//! the fetcher is a policy-checked `find_many` by primary key, and the
//! default mutators are the model's create/update/delete delegates.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::{CratestackContext, CratestackError};
use cratestack_sqlx::sqlx::PgTransaction;
use serde_json::Value;
use uuid::Uuid;

use crate::protocol::RejectCode;

/// Reads the current state of rows a puller was told about.
#[async_trait]
pub trait RowFetcher: Send + Sync {
    /// Return each row in `pks` that the caller may read *now*, keyed by its
    /// primary key as text, serialized exactly as the client's generated
    /// model deserializes it. Omit a row that is gone, soft-deleted, or not
    /// readable under `ctx` — the puller receives a delete for it.
    ///
    /// Implementations must apply the model's read policy (use the
    /// generated delegate with `ctx`, not raw SQL). This is the second of
    /// two independent checks: scopes decide what a client is *told*
    /// about, the read policy decides what it *receives*.
    async fn fetch(
        &self,
        ctx: &CratestackContext,
        pks: &[String],
    ) -> Result<HashMap<String, Value>, CratestackError>;
}

/// Who is pushing, handed to every [`MutationHandler`].
#[derive(Debug, Clone, Copy)]
pub struct MutationContext<'a> {
    pub ctx: &'a CratestackContext,
    pub principal: &'a str,
    pub client_id: Uuid,
    pub seq: u64,
}

/// Why a mutation was not applied.
#[derive(Debug)]
pub enum MutationError {
    /// Consumed and reported to the client; the push continues.
    Reject(RejectCode, String),
    /// Infrastructure failure; the whole push rolls back and the client
    /// retries the same seqs later.
    Fatal(CratestackError),
}

impl From<CratestackError> for MutationError {
    fn from(error: CratestackError) -> Self {
        let message = error.to_string();
        match error {
            CratestackError::PreconditionFailed(_)
            | CratestackError::Conflict(_)
            | CratestackError::ConflictTyped(_) => Self::Reject(RejectCode::Conflict, message),
            CratestackError::Forbidden(_) | CratestackError::Unauthorized(_) => {
                Self::Reject(RejectCode::Forbidden, message)
            }
            CratestackError::NotFound(_) => Self::Reject(RejectCode::NotFound, message),
            CratestackError::Validation(_) | CratestackError::BadRequest(_) => {
                Self::Reject(RejectCode::Invalid, message)
            }
            other => Self::Fatal(other),
        }
    }
}

impl From<serde_json::Error> for MutationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Reject(RejectCode::Invalid, format!("invalid args: {error}"))
    }
}

/// Applies one named mutation inside the push transaction. Each call runs
/// in its own savepoint, so a rejected mutation leaves no partial writes.
///
/// `args` are untrusted. Write through the generated delegates'
/// `run_in_tx(tx, mc.ctx)` so the model's create/update/delete policies,
/// validators and `@version` checks apply exactly as on any other route.
#[async_trait]
pub trait MutationHandler: Send + Sync {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError>;
}

/// Registered fetchers (by model name) and mutators (by mutation name).
#[derive(Clone, Default)]
pub struct SyncRegistry {
    pub(crate) fetchers: HashMap<String, Arc<dyn RowFetcher>>,
    pub(crate) mutators: HashMap<String, Arc<dyn MutationHandler>>,
}

impl SyncRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn model(mut self, model: impl Into<String>, fetcher: impl RowFetcher + 'static) -> Self {
        self.fetchers.insert(model.into(), Arc::new(fetcher));
        self
    }

    pub fn mutation(
        mut self,
        name: impl Into<String>,
        handler: impl MutationHandler + 'static,
    ) -> Self {
        self.mutators.insert(name.into(), Arc::new(handler));
        self
    }
}
