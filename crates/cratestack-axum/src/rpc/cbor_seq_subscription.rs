//! The `application/cbor-seq` subscription binding
//! (`docs/design/subscriptions-websocket.md` §6a.3, cratestack#1001): the
//! SSE binding's frames (ids counting up per frame), CBOR-encoded, as an HTTP
//! response body. WebTransport differs: it carries one subscription id.
//!
//! Each item of the CBOR sequence (RFC 8742) is one
//! [`ServerFrame`]. First `{id: 1, ready: {}}`: the subscription is live and
//! nothing committed after this point will be missed. Then
//! `{id: n, next: <item>}` per event, with `n` counting up from 2 as on SSE.
//! The stream ends with exactly one terminal frame: `{id, err}` for an error
//! (a model feed's overflow arrives as `unavailable`), or `{id, end: {}}` when
//! the source simply ran out. While no item is due, a bare CBOR `null`
//! (`0xf6`) goes out every 15 s, so an idle proxy leaves the connection
//! open; it is not a frame, and a reader skips it.
//!
//! Frames go through [`CborCodec`], the codec WebSocket and WebTransport use,
//! so a `Uuid` or `Bytes` item is a byte string, as in a unary CBOR response.

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{CratestackCodec, CratestackError, SubscriptionStream};
use futures_util::future::{Either, select};
use futures_util::stream::{self, StreamExt};
use serde::Serialize;

use super::sse_subscription::KEEPALIVE;
use crate::transport::{CBOR_SEQUENCE_CONTENT_TYPE, StreamedResponseMarker};

/// A CBOR `null`: the keep-alive item.
const KEEPALIVE_ITEM: &[u8] = &[0xf6];

/// Encode a subscription's items as an `application/cbor-seq` response.
pub fn encode_subscription_cbor_seq_response<T: Serialize + Send + 'static>(
    items: SubscriptionStream<T>,
) -> Response {
    let ready = stream::once(async { frame(&ServerFrame::ready(1)) });
    let items = stream::unfold((items, 1u64, false), |(mut items, id, ended)| async move {
        if ended {
            return None;
        }
        let next = match select(items.next(), Box::pin(tokio::time::sleep(KEEPALIVE))).await {
            Either::Left((next, _)) => next,
            Either::Right(((), _)) => {
                return Some((Ok(Bytes::from_static(KEEPALIVE_ITEM)), (items, id, false)));
            }
        };
        let id = id + 1;
        let (bytes, ended) = match next {
            Some(Ok(item)) => match RpcFrameValue::encode_with(&CborCodec, &item) {
                Ok(value) => (frame(&ServerFrame::next(id, value)), false),
                Err(error) => (frame(&ServerFrame::err(id, &error)), true),
            },
            Some(Err(error)) => (frame(&ServerFrame::err(id, &error)), true),
            None => (frame(&ServerFrame::end(id)), true),
        };
        Some((bytes, (items, id, ended)))
    });
    let mut response = Response::new(Body::from_stream(ready.chain(items)));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(CBOR_SEQUENCE_CONTENT_TYPE),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // As the SSE response: a body that must not be buffered whole by a layer
    // over the router (`crate::transport::StreamedResponseMarker`).
    response.extensions_mut().insert(StreamedResponseMarker);
    response
}

/// One frame's CBOR. A frame is a small fixed shape, so this failing is a
/// codec bug; it aborts the body, which a reader sees as a truncated stream
/// (an error), never as a clean end or a silent gap.
fn frame(frame: &ServerFrame) -> Result<Bytes, CratestackError> {
    CborCodec.encode(frame).map(Bytes::from)
}

#[cfg(test)]
mod tests;
