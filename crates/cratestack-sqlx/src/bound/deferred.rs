//! What an attempt queues to do only once it commits: its audit events, the
//! outbox drain and the topic messages of a backplane a transaction cannot
//! notify. A rolled-back joined call takes back what it queued
//! ([`BoundTx::mark`], [`BoundTx::truncate`]); a retried attempt starts
//! with a fresh `BoundTx`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cratestack_core::{AuditEvent, SubscriptionBackplane};

use super::BoundTx;

/// A topic message for a backplane that cannot notify inside a transaction.
pub(crate) struct DeferredPublish {
    backplane: Arc<dyn SubscriptionBackplane>,
    channel: String,
    payload: String,
}

/// What a committed attempt still has to do.
pub(crate) struct Deferred {
    pub(crate) audit_events: Vec<AuditEvent>,
    pub(crate) drain_outbox: bool,
    publishes: Vec<DeferredPublish>,
}

impl Deferred {
    /// Sends the queued topic messages, in order. Best effort: the attempt
    /// has committed, so a failure is logged, as the `AuditSink`'s is.
    pub(crate) async fn publish(self) {
        for DeferredPublish {
            backplane,
            channel,
            payload,
        } in self.publishes
        {
            if let Err(error) = backplane.publish(&channel, &payload).await {
                tracing::warn!(
                    target: "cratestack",
                    cratestack_error = error.code(),
                    cratestack_channel = channel,
                    "a topic message queued by an @isolation procedure could not be published",
                );
            }
        }
    }
}

/// How much is queued, to drop what a rolled-back joined call queued.
pub(super) struct Mark {
    audit_events: usize,
    publishes: usize,
}

impl BoundTx {
    /// Queue a topic message to be sent when the attempt commits.
    pub(crate) fn defer_publish(
        &self,
        backplane: Arc<dyn SubscriptionBackplane>,
        channel: String,
        payload: String,
    ) {
        if let Ok(mut queued) = self.publishes.lock() {
            queued.push(DeferredPublish {
                backplane,
                channel,
                payload,
            });
        }
    }

    pub(crate) fn take_deferred(&self) -> Deferred {
        let audit_events = self
            .audit_events
            .lock()
            .map(|mut queued| std::mem::take(&mut *queued))
            .unwrap_or_default();
        let publishes = self
            .publishes
            .lock()
            .map(|mut queued| std::mem::take(&mut *queued))
            .unwrap_or_default();
        Deferred {
            audit_events,
            drain_outbox: self.drain_outbox.load(Ordering::SeqCst),
            publishes,
        }
    }

    pub(super) fn mark(&self) -> Mark {
        Mark {
            audit_events: self.audit_events.lock().map_or(0, |queued| queued.len()),
            publishes: self.publishes.lock().map_or(0, |queued| queued.len()),
        }
    }

    pub(super) fn truncate(&self, mark: Mark) {
        if let Ok(mut queued) = self.audit_events.lock() {
            queued.truncate(mark.audit_events);
        }
        if let Ok(mut queued) = self.publishes.lock() {
            queued.truncate(mark.publishes);
        }
    }
}
