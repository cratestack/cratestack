mod event_outbox;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::sqlx;

use cratestack_core::{
    AuditSink, CratestackError, CratestackEventBus, CratestackEventEnvelope, CratestackEventFuture,
    ModelEventKind, NoopAuditSink, SubscriptionHandle,
};

use crate::error::cool_error_from_sqlx;
use event_outbox::EventOutboxRow;

pub use event_outbox::{enqueue_event_outbox, ensure_event_outbox_table};

#[derive(Clone)]
pub struct SqlxRuntime {
    pool: sqlx::PgPool,
    events: CratestackEventBus,
    // Shared (not per-clone) so every handle onto the same logical
    // runtime agrees on whether `cratestack_audit` has been
    // bootstrapped. See `crate::audit::ensure_audit_table` — this is
    // what lets it skip re-issuing `CREATE INDEX IF NOT EXISTS` after
    // the first call, which is what self-deadlocked chained
    // `run_in_tx` audit writes in a caller-managed transaction.
    audit_table_ensured: Arc<AtomicBool>,
    // Installation point for cratestack#473: defaults to `NoopAuditSink`
    // so existing callers of `new()` see no behavior change. Installed
    // via `with_audit_sink` (mirrors `IdempotencyLayer::new`/
    // `with_principal_fingerprint`'s builder shape). The DB write in
    // `crate::audit::enqueue_audit_event` remains the sole source of
    // truth; this is a best-effort downstream projection dispatched
    // from `crate::audit::dispatch_audit_sink` after the owning
    // transaction commits.
    audit_sink: Arc<dyn AuditSink>,
}

// `dyn AuditSink` has no `Debug` bound (matching `IdempotencyStore` /
// `RateLimitStore`, neither of which require one either), so this can't
// be `#[derive(Debug)]` — same reason `CratestackEventBus` hand-rolls its own
// `Debug` impl instead of deriving one.
impl std::fmt::Debug for SqlxRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlxRuntime")
            .field("pool", &self.pool)
            .field("events", &self.events)
            .field(
                "audit_table_ensured",
                &self
                    .audit_table_ensured
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl SqlxRuntime {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            events: CratestackEventBus::default(),
            audit_table_ensured: Arc::new(AtomicBool::new(false)),
            audit_sink: Arc::new(NoopAuditSink),
        }
    }

    pub fn pool(&self) -> &sqlx::PgPool {
        &self.pool
    }

    pub(crate) fn audit_table_ensured(&self) -> &AtomicBool {
        &self.audit_table_ensured
    }

    /// Install a custom [`AuditSink`] that every `@@audit` mutation on
    /// this runtime fans out to, in addition to the in-database
    /// `cratestack_audit` table row `enqueue_audit_event` always
    /// writes. Composable via [`cratestack_core::MulticastAuditSink`]
    /// for more than one downstream (Kafka, Redis pubsub, a webhook —
    /// this crate ships none of them; see `AuditSink`'s doc comment).
    pub fn with_audit_sink(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = sink;
        self
    }

    pub(crate) fn audit_sink(&self) -> &Arc<dyn AuditSink> {
        &self.audit_sink
    }

    #[doc(hidden)]
    pub fn subscribe<F>(
        &self,
        model: &'static str,
        operation: ModelEventKind,
        handler: F,
    ) -> SubscriptionHandle
    where
        F: Fn(CratestackEventEnvelope) -> CratestackEventFuture + Send + Sync + 'static,
    {
        self.events.subscribe(model, operation, handler)
    }

    /// An owned, cheaply-cloneable handle onto the underlying
    /// `CratestackEventBus` — needed by callers (e.g. `@@subscribe` SSE
    /// dispatch, cratestack#390) that outlive the `&SqlxRuntime` borrow
    /// `subscribe`/`unsubscribe` would otherwise require.
    #[doc(hidden)]
    pub fn events_bus(&self) -> CratestackEventBus {
        self.events.clone()
    }

    #[doc(hidden)]
    pub async fn drain_event_outbox(&self) -> Result<usize, CratestackError> {
        ensure_event_outbox_table(&self.pool).await?;

        let rows = sqlx::query_as::<_, EventOutboxRow>(
            "SELECT event_id, model, operation, occurred_at, payload, attempts, last_error \
             FROM cratestack_event_outbox \
             WHERE delivered_at IS NULL \
             ORDER BY occurred_at ASC, event_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(cool_error_from_sqlx)?;

        let mut delivered = 0usize;
        for row in rows {
            let event_id = row.event_id;
            let envelope = row.try_into_envelope()?;
            match self.events.emit(envelope).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET delivered_at = NOW(), last_error = NULL, attempts = attempts + 1 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .execute(&self.pool)
                    .await
                    .map_err(cool_error_from_sqlx)?;
                    delivered += 1;
                }
                Err(error) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET attempts = attempts + 1, last_error = $2 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .bind(error.to_string())
                    .execute(&self.pool)
                    .await
                    .map_err(cool_error_from_sqlx)?;
                }
            }
        }

        Ok(delivered)
    }
}
