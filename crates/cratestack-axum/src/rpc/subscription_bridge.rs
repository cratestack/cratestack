//! Bridges a `CoolEventBus`-style push callback into a bounded,
//! backpressure-aware `Stream` for the SSE encoder ([`super::sse`]). See
//! `docs/design/rpc-transport.md` §3.4/§3.4a: "bounded per-subscription
//! send buffer; on overflow, emit Error{unavailable} ... and end the
//! stream."
//!
//! `CoolEventBus::subscribe` handlers must never block or fail the
//! emitting transaction just because one particular SSE client is slow
//! to drain, so the push side here is always non-blocking (`try_send`)
//! and infallible from the bus's point of view — overflow is signaled to
//! the *consumer* by closing the channel, never back to the caller of
//! `emit()`. See [`super::sse`]'s module doc for why "the stream ends"
//! is therefore an unambiguous overflow signal, never confused with an
//! ordinary client disconnect (which just drops the whole future
//! instead of closing this channel).

use std::sync::{Arc, Mutex};

use futures_util::Stream;
use futures_util::stream;
use tokio::sync::mpsc;

/// One SSE subscription's outbox capacity before it's considered lagged.
/// A slow consumer past this many buffered, undelivered events gets
/// disconnected with `Error{unavailable}` (see [`super::sse`]) rather
/// than growing memory unboundedly.
const SUBSCRIPTION_BUFFER_CAPACITY: usize = 64;

/// Handed to one or more `CoolEventBus::subscribe` callbacks registered
/// for the same logical subscription (e.g. one per `@@emit`ted operation
/// on a model). Every clone shares the same underlying sender slot, so
/// the *first* overflow observed by any of them permanently closes the
/// channel — subsequent pushes from any clone become silent no-ops.
pub struct SubscriptionPush<T> {
    slot: Arc<Mutex<Option<mpsc::Sender<T>>>>,
}

impl<T> Clone for SubscriptionPush<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
        }
    }
}

impl<T: Send + 'static> SubscriptionPush<T> {
    /// Non-blocking push. Never fails from the caller's perspective —
    /// overflow just closes the slot so this and every other clone
    /// becomes a no-op from then on; the *consumer* observes that as the
    /// stream ending (see [`guarded_receiver_stream`]).
    pub fn push(&self, value: T) {
        let sender = {
            let guard = self.slot.lock().expect("subscription sender slot poisoned");
            guard.clone()
        };
        let Some(sender) = sender else {
            return;
        };
        if sender.try_send(value).is_err() {
            // Either full (backpressure) or the receiver already
            // dropped (client disconnected without ever overflowing —
            // harmless to also close here, `guarded_receiver_stream`'s
            // future was already being torn down for that case anyway).
            *self.slot.lock().expect("subscription sender slot poisoned") = None;
        }
    }
}

/// Builds a fresh bounded channel plus the [`SubscriptionPush`] handle
/// callers clone into every `CoolEventBus::subscribe` closure that
/// should feed it.
pub fn subscription_channel<T: Send + 'static>() -> (SubscriptionPush<T>, mpsc::Receiver<T>) {
    let (tx, rx) = mpsc::channel(SUBSCRIPTION_BUFFER_CAPACITY);
    (
        SubscriptionPush {
            slot: Arc::new(Mutex::new(Some(tx))),
        },
        rx,
    )
}

/// Wraps a raw `mpsc::Receiver` into a `Stream`, keeping `guard` alive
/// for exactly as long as the stream is — dropped together whether the
/// stream ends normally (overflow, see [`SubscriptionPush`]) or is
/// cancelled mid-poll (an ordinary client disconnect just drops this
/// whole future). This is what lets a `cratestack_core::SubscriptionGuard`
/// passed as `guard` unsubscribe cleanly in either case without the
/// caller needing to distinguish which one happened.
pub fn guarded_receiver_stream<T, G>(
    rx: mpsc::Receiver<T>,
    guard: G,
) -> impl Stream<Item = T> + Send + 'static
where
    T: Send + 'static,
    G: Send + 'static,
{
    stream::unfold((rx, guard), |(mut rx, guard)| async move {
        let item = rx.recv().await?;
        Some((item, (rx, guard)))
    })
}

#[cfg(test)]
mod tests;
