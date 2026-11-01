mod drain;
mod event_outbox;
mod outbox_read;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::sqlx;

use cratestack_core::{
    AuditSink, CratestackEventBus, CratestackEventEnvelope, CratestackEventFuture, ModelEventKind,
    NoopAuditSink, SubscriptionBackplane, SubscriptionHandle,
};

pub(crate) use event_outbox::enqueue_model_event;
pub use event_outbox::{enqueue_event_outbox, ensure_event_outbox_table};
pub(crate) use outbox_read::outbox_envelope_json;

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
    // `Some` only on the per-attempt runtime `run_isolated` builds for an
    // `@isolation` procedure: every executing path then runs on this
    // transaction instead of the pool (docs/design/procedure-isolation.md §4).
    pub(crate) bound: Option<Arc<crate::bound::BoundTx>>,
    pub(crate) isolation_max_retries: u32,
    // Where model events reach subscribers on every replica
    // (docs/design/subscriptions-websocket.md §4). `PgNotifyBackplane` by
    // default; `with_backplane` swaps it (an `InProcessBackplane` for a
    // single replica behind a pooler that drops `LISTEN`).
    backplane: Arc<dyn SubscriptionBackplane>,
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
            .field("bound", &self.bound.is_some())
            .field("isolation_max_retries", &self.isolation_max_retries)
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
        let backplane: Arc<dyn SubscriptionBackplane> =
            Arc::new(crate::backplane::PgNotifyBackplane::new(pool.clone()));
        Self {
            pool,
            events: CratestackEventBus::default(),
            audit_table_ensured: Arc::new(AtomicBool::new(false)),
            audit_sink: Arc::new(NoopAuditSink),
            bound: None,
            isolation_max_retries: crate::isolation::MAX_RETRIES_DEFAULT,
            backplane,
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

    /// Replace the subscription backplane (default: `PgNotifyBackplane` on
    /// this runtime's pool).
    pub fn with_backplane(mut self, backplane: Arc<dyn SubscriptionBackplane>) -> Self {
        self.backplane = backplane;
        self
    }

    pub fn backplane(&self) -> &Arc<dyn SubscriptionBackplane> {
        &self.backplane
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
}
