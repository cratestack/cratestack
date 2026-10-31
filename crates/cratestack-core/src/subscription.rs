//! The transport-neutral half of a subscription
//! (`docs/design/subscriptions-websocket.md` §6.1).
//!
//! A binding — SSE, WebSocket, WebTransport — only moves frames. Opening a
//! subscription (policy, setup, backplane registration) and producing its
//! items is the [`SubscriptionDispatcher`]'s job, so every binding enforces
//! exactly the same thing and none of them carries its own copy of the
//! policy code.
//!
//! Values cross this boundary as `serde_json::Value`, the same opaque
//! codec value `RpcRequest::input` / `RpcResponseFrame::output` already use:
//! the binding decodes its wire codec into it and encodes items back out,
//! so a dispatcher never knows which codec or transport it is serving.

use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;

use crate::context::CratestackContext;
use crate::error::CratestackError;

/// A live subscription's items. Ending (`None`) means the subscription is
/// over; an `Err` item is terminal too — the binding sends it and stops.
pub type SubscriptionStream<T> =
    Pin<Box<dyn Stream<Item = Result<T, CratestackError>> + Send + 'static>>;

/// Everything a dispatcher needs to open one subscription. Owned, so the
/// future [`SubscriptionDispatcher::open`] returns borrows nothing from the
/// binding's connection state.
#[derive(Debug, Clone)]
pub struct SubscriptionOpen {
    /// Dotted op id, e.g. `"model.Widget.subscribe"` or
    /// `"procedure.roomMessages"`.
    pub op_id: String,
    /// The op's input, decoded from the binding's codec.
    pub input: serde_json::Value,
    /// The authenticated caller. Captured once, at open, and used for every
    /// item of this subscription (design §5.1).
    pub ctx: CratestackContext,
}

/// Opens subscriptions by op id. Implemented by generated code (one impl
/// per schema); called by every subscription binding.
///
/// Shaped like [`crate::AuthProvider`]: implement `open` as a plain
/// `async fn`. Returning `Err` refuses the subscription before the binding
/// tells the client it is ready, so policy refusals and setup failures
/// ("room not found") never look like a mid-stream error.
pub trait SubscriptionDispatcher: Clone + Send + Sync + 'static {
    fn open(
        &self,
        request: SubscriptionOpen,
    ) -> impl Future<Output = Result<SubscriptionStream<serde_json::Value>, CratestackError>> + Send;
}
