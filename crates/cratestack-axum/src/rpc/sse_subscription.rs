//! The SSE binding as an adapter over a [`SubscriptionStream`]
//! (`docs/design/subscriptions-websocket.md` §6.1, slice S5a): the
//! generated `GET /rpc/subscribe/{op_id}` handler opens the subscription
//! through the schema's `SubscriptionDispatcher` and hands the stream here.
//!
//! The wire is [`super::sse`]'s, unchanged: one `event: message` per item
//! (`{"id": <n>, "next": <item>}`), and the stream's terminal error as one
//! `event: error` (`{"id": <n>, "err": RpcErrorBody}`). A model feed's
//! overflow arrives as that error (`unavailable` / `"subscription
//! lagged"`), so it reads exactly as it always has. A stream that simply
//! ends sends nothing more.

use axum::body::{Body, Bytes};
use axum::response::Response;
use cratestack_core::SubscriptionStream;
use cratestack_core::rpc::RpcErrorBody;
use futures_util::stream::{self, StreamExt};

use super::sse::{format_error_event, format_message_event, sse_response};

/// Encode a subscription's items as a `text/event-stream` response.
pub fn encode_subscription_sse_response(items: SubscriptionStream<serde_json::Value>) -> Response {
    let body = stream::unfold((items, 0u64, false), |(mut items, id, ended)| async move {
        if ended {
            return None;
        }
        let id = id + 1;
        let (bytes, ended): (Bytes, bool) = match items.next().await? {
            Ok(item) => (format_message_event(id, &item), false),
            Err(error) => (
                format_error_event(id, &RpcErrorBody::from_cratestack(&error)),
                true,
            ),
        };
        Some((Ok::<_, std::convert::Infallible>(bytes), (items, id, ended)))
    });
    sse_response(Body::from_stream(body))
}

#[cfg(test)]
mod tests;
