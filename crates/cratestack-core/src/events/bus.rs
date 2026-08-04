//! [`CoolEventBus`] itself: the in-process pub/sub registry
//! `emit`/`subscribe` operate on, plus [`SubscriptionHandle`] /
//! [`SubscriptionGuard`] for removing a registered handler again —
//! needed once a subscription's lifecycle is shorter than the process's
//! (e.g. one `GET /rpc/subscribe/{op_id}` connection, §3.4a).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use super::{CoolEventEnvelope, CoolEventFuture, ModelEventKind, event_topic};
use crate::error::CoolError;

type EventHandler = Arc<dyn Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync>;

/// Opaque token returned by [`CoolEventBus::subscribe`], needed to later
/// remove that exact handler via [`CoolEventBus::unsubscribe`]. Fields
/// are private — the only way to obtain one is `subscribe`, and the only
/// thing it's good for is passing back to `unsubscribe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionHandle {
    topic: String,
    id: u64,
}

#[derive(Clone, Default)]
pub struct CoolEventBus {
    handlers: Arc<RwLock<BTreeMap<String, Vec<(u64, EventHandler)>>>>,
    next_id: Arc<AtomicU64>,
}

impl std::fmt::Debug for CoolEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let handler_count = self
            .handlers
            .read()
            .map(|handlers| handlers.values().map(Vec::len).sum::<usize>())
            .unwrap_or_default();
        f.debug_struct("CoolEventBus")
            .field("handler_count", &handler_count)
            .finish()
    }
}

impl CoolEventBus {
    pub fn subscribe<F>(
        &self,
        model: &'static str,
        operation: ModelEventKind,
        handler: F,
    ) -> SubscriptionHandle
    where
        F: Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync + 'static,
    {
        let topic = event_topic(model, operation);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut handlers = self
            .handlers
            .write()
            .expect("event bus handler registry should not be poisoned");
        handlers
            .entry(topic.clone())
            .or_default()
            .push((id, Arc::new(handler)));
        SubscriptionHandle { topic, id }
    }

    /// Removes the handler registered by a prior [`Self::subscribe`]
    /// call. A no-op if the handle's topic/id pair is no longer present
    /// (already removed, or from a different `CoolEventBus` instance) —
    /// callers don't need to track whether they've already unsubscribed.
    pub fn unsubscribe(&self, handle: SubscriptionHandle) {
        let mut handlers = self
            .handlers
            .write()
            .expect("event bus handler registry should not be poisoned");
        if let Some(topic_handlers) = handlers.get_mut(&handle.topic) {
            topic_handlers.retain(|(id, _)| *id != handle.id);
        }
    }

    pub async fn emit(&self, envelope: CoolEventEnvelope) -> Result<(), CoolError> {
        let handlers: Vec<EventHandler> = self
            .handlers
            .read()
            .expect("event bus handler registry should not be poisoned")
            .get(&event_topic(&envelope.model, envelope.operation))
            .map(|entries| entries.iter().map(|(_, handler)| handler.clone()).collect())
            .unwrap_or_default();

        for handler in handlers {
            handler(envelope.clone()).await?;
        }

        Ok(())
    }
}

/// RAII cleanup for one or more [`CoolEventBus`] subscriptions that all
/// share one lifecycle — e.g. the per-operation handlers a single
/// `GET /rpc/subscribe/{op_id}` connection registers for the duration of
/// its SSE stream (`docs/design/rpc-transport.md` §3.4a, cratestack#390).
/// Every tracked handle is unsubscribed when the guard drops, whether
/// that's because the underlying stream ended normally (backpressure
/// overflow) or because it was cancelled mid-poll (an ordinary client
/// disconnect) — both just drop this guard the same way, so cleanup
/// doesn't need to special-case which one happened. Without this, a
/// long-running server would accumulate one permanently-registered,
/// permanently-a-no-op handler per historical connection — a real
/// unbounded-memory footgun for a public, freely-reconnectable endpoint,
/// not a hypothetical one.
#[derive(Default)]
pub struct SubscriptionGuard {
    bus: Option<CoolEventBus>,
    handles: Vec<SubscriptionHandle>,
}

impl SubscriptionGuard {
    pub fn new(bus: CoolEventBus) -> Self {
        Self {
            bus: Some(bus),
            handles: Vec::new(),
        }
    }

    /// Adds a handle to the set this guard unsubscribes on drop.
    pub fn track(&mut self, handle: SubscriptionHandle) {
        self.handles.push(handle);
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let Some(bus) = &self.bus else {
            return;
        };
        for handle in self.handles.drain(..) {
            bus.unsubscribe(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::events::ModelEvent;

    fn envelope(model: &str, operation: ModelEventKind) -> CoolEventEnvelope {
        CoolEventEnvelope {
            event_id: uuid::Uuid::new_v4(),
            model: model.to_owned(),
            operation,
            occurred_at: chrono::Utc::now(),
            data: serde_json::json!({"id": 1}),
        }
    }

    #[tokio::test]
    async fn unsubscribe_stops_further_delivery() {
        let bus = CoolEventBus::default();
        let received = Arc::new(Mutex::new(0u32));
        let received_clone = Arc::clone(&received);
        let handle = bus.subscribe("Widget", ModelEventKind::Created, move |_event| {
            let received = Arc::clone(&received_clone);
            Box::pin(async move {
                *received.lock().unwrap() += 1;
                Ok(())
            })
        });

        bus.emit(envelope("Widget", ModelEventKind::Created))
            .await
            .unwrap();
        assert_eq!(*received.lock().unwrap(), 1);

        bus.unsubscribe(handle);

        bus.emit(envelope("Widget", ModelEventKind::Created))
            .await
            .unwrap();
        assert_eq!(
            *received.lock().unwrap(),
            1,
            "no further delivery after unsubscribe"
        );
    }

    #[tokio::test]
    async fn unsubscribe_does_not_affect_other_handlers_on_the_same_topic() {
        let bus = CoolEventBus::default();
        let count_a = Arc::new(Mutex::new(0u32));
        let count_b = Arc::new(Mutex::new(0u32));

        let handle_a = {
            let count_a = Arc::clone(&count_a);
            bus.subscribe("Widget", ModelEventKind::Created, move |_event| {
                let count_a = Arc::clone(&count_a);
                Box::pin(async move {
                    *count_a.lock().unwrap() += 1;
                    Ok(())
                })
            })
        };
        {
            let count_b = Arc::clone(&count_b);
            bus.subscribe("Widget", ModelEventKind::Created, move |_event| {
                let count_b = Arc::clone(&count_b);
                Box::pin(async move {
                    *count_b.lock().unwrap() += 1;
                    Ok(())
                })
            });
        }

        bus.unsubscribe(handle_a);
        bus.emit(envelope("Widget", ModelEventKind::Created))
            .await
            .unwrap();

        assert_eq!(*count_a.lock().unwrap(), 0);
        assert_eq!(*count_b.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn unsubscribe_is_a_no_op_for_an_unknown_handle() {
        let bus = CoolEventBus::default();
        // Never subscribed anywhere; must not panic.
        bus.unsubscribe(SubscriptionHandle {
            topic: "Widget.created".to_owned(),
            id: 42,
        });
    }

    #[tokio::test]
    async fn subscription_guard_unsubscribes_every_tracked_handle_on_drop() {
        let bus = CoolEventBus::default();
        let received = Arc::new(Mutex::new(0u32));

        let mut guard = SubscriptionGuard::new(bus.clone());
        for _ in 0..2 {
            let received = Arc::clone(&received);
            let handle = bus.subscribe("Widget", ModelEventKind::Created, move |_event| {
                let received = Arc::clone(&received);
                Box::pin(async move {
                    *received.lock().unwrap() += 1;
                    Ok(())
                })
            });
            guard.track(handle);
        }

        bus.emit(envelope("Widget", ModelEventKind::Created))
            .await
            .unwrap();
        assert_eq!(*received.lock().unwrap(), 2, "both handlers fired");

        drop(guard);

        bus.emit(envelope("Widget", ModelEventKind::Created))
            .await
            .unwrap();
        assert_eq!(
            *received.lock().unwrap(),
            2,
            "dropping the guard unsubscribed both handlers"
        );
    }

    #[test]
    fn model_event_try_from_envelope_still_works_alongside_the_bus_types() {
        // Sanity check that this submodule split didn't break the
        // sibling `ModelEvent`/`TryFrom` path in `events.rs`.
        #[derive(serde::Deserialize)]
        struct Widget {
            id: i64,
        }
        let event =
            ModelEvent::<Widget>::try_from(envelope("Widget", ModelEventKind::Created)).unwrap();
        assert_eq!(event.data.id, 1);
    }
}
