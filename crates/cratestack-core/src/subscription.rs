//! The transport-neutral half of a subscription
//! (`docs/design/subscriptions-websocket.md` §6.1).
//!
//! A binding — SSE, WebSocket, WebTransport — only moves frames. Opening a
//! subscription (policy, setup, backplane registration) and producing its
//! items is the [`SubscriptionDispatcher`]'s job, so every binding enforces
//! exactly the same thing and none of them carries its own copy of the
//! policy code.
//!
//! Items cross this boundary as [`RpcFrameValue`], a JSON-shaped value with
//! a byte-string arm (the payload of a batch frame too). The dispatcher is
//! handed the binding's codec and encodes each item with it, so a `Uuid` or
//! `Bytes` is a byte string on a CBOR binding and text or an integer array
//! on a JSON one, exactly as in a unary response of that codec
//! (cratestack#1360). The binding decodes its wire into the same value and
//! writes it back out, so a dispatcher never knows which transport it is
//! serving.

use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;

use crate::codec::CratestackCodec;
use crate::context::CratestackContext;
use crate::error::CratestackError;
use crate::rpc::RpcFrameValue;

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
    /// The op's input, decoded from the binding's codec. It becomes a JSON
    /// query string either way, so it stays JSON-shaped.
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
///
/// `codec` is the binding's wire codec (SSE passes the JSON one). Each item
/// is encoded with it ([`RpcFrameValue::encode_with`]), so the frame an item
/// ends up in matches what a unary call of that codec would return.
pub trait SubscriptionDispatcher: Clone + Send + Sync + 'static {
    fn open<C: CratestackCodec>(
        &self,
        request: SubscriptionOpen,
        codec: C,
    ) -> impl Future<Output = Result<SubscriptionStream<RpcFrameValue>, CratestackError>> + Send;
}
