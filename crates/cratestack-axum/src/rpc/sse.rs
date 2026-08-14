//! SSE encoding for `@@subscribe` model-event streams (design doc
//! §3.4a, cratestack#390). Counterpart to
//! `crate::transport::stream_sequence`'s `application/cbor-seq` encoder
//! for `@stream` procedures (§3.3) — same "encode item-by-item onto
//! `axum::body::Body::from_stream`, never buffer the whole response"
//! shape, but framed as `text/event-stream` instead of raw CBOR
//! concatenation.
//!
//! Wire format: one `event: message` per item (`data:` is a JSON object
//! `{"id": <u64>, "next": <item>}`, mirroring §2.3's `StreamItem` frame),
//! and exactly one final `event: error` (`data:` is `{"id": <u64>,
//! "err": RpcErrorBody}`) when the underlying stream ends — see
//! `super::subscription_bridge`'s module doc for why "the stream ends"
//! only ever means backpressure overflow here, never an ordinary client
//! disconnect (which just drops this whole future instead, so this code
//! never runs for that case).
//!
//! Payload encoding is always JSON regardless of which `CratestackCodec` the
//! server negotiates for its unary/batch RPC routes: SSE is a
//! text-based wire format by construction, and JSON is already one of
//! the two codecs this framework's RPC binding supports
//! (`RPC_BINDING_CAPABILITIES`) — there's no reason to invent a
//! base64-wrapped-CBOR convention nobody asked for when every
//! off-the-shelf `EventSource` client and `curl` already expects JSON
//! text bodies over SSE.

use std::pin::Pin;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcErrorBody;
use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use serde::Serialize;

use crate::transport::StreamedResponseMarker;

const SSE_CONTENT_TYPE: &str = "text/event-stream";

/// `GET /rpc/subscribe/{op_id}` has no upgrade handshake to negotiate a
/// binary subprotocol like WS does — the client states its intent via a
/// plain `Accept` header, same as every other HTTP RPC binding. Reject
/// anything that doesn't ask for SSE up front, before any
/// `CratestackEventBus` subscription gets registered.
pub fn validate_subscribe_accept_header(headers: &HeaderMap) -> Result<(), CratestackError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Err(CratestackError::NotAcceptable(format!(
            "subscription endpoint requires Accept: {SSE_CONTENT_TYPE}"
        )));
    };
    let accept = accept
        .to_str()
        .map_err(|error| CratestackError::BadRequest(format!("invalid Accept header: {error}")))?;
    if accept
        .split(',')
        .map(str::trim)
        .any(|value| value == SSE_CONTENT_TYPE || value == "*/*")
    {
        Ok(())
    } else {
        Err(CratestackError::NotAcceptable(format!(
            "subscription endpoint requires Accept: {SSE_CONTENT_TYPE}, got {accept}"
        )))
    }
}

/// Encode `items` as a `text/event-stream` response. `items` ending
/// (`None` from the underlying `Stream`) always means backpressure
/// overflow closed the channel (see module docs) — the last byte chunk
/// written is always the `Error{unavailable}` sentinel event.
pub fn encode_model_event_sse_response<T, S>(items: S) -> Response
where
    T: Serialize + Send + 'static,
    S: Stream<Item = T> + Send + 'static,
{
    let mut response = Response::new(Body::from_stream(encode_sse_events(items)));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(SSE_CONTENT_TYPE),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // See `crate::transport::StreamedResponseMarker` — bypasses any
    // response-buffering middleware (e.g. `IdempotencyLayer`) an
    // embedding app layers over the whole router, the same way the
    // `@stream` cbor-seq encoder already does.
    response.extensions_mut().insert(StreamedResponseMarker);
    response
}

struct EventState<S> {
    source: Pin<Box<S>>,
    id: u64,
    ended: bool,
}

fn encode_sse_events<T, S>(
    items: S,
) -> impl Stream<Item = Result<Bytes, std::convert::Infallible>> + Send
where
    T: Serialize + Send + 'static,
    S: Stream<Item = T> + Send + 'static,
{
    let initial = EventState {
        source: Box::pin(items),
        id: 0,
        ended: false,
    };
    stream::unfold(initial, |mut state| async move {
        if state.ended {
            return None;
        }
        match state.source.next().await {
            Some(item) => {
                state.id += 1;
                let bytes = format_message_event(state.id, &item);
                Some((Ok(bytes), state))
            }
            None => {
                state.ended = true;
                state.id += 1;
                let bytes = format_error_event(state.id, &lagged_error());
                Some((Ok(bytes), state))
            }
        }
    })
}

fn lagged_error() -> RpcErrorBody {
    RpcErrorBody::from_cratestack(&CratestackError::Unavailable(
        "subscription lagged".to_owned(),
    ))
}

#[derive(Serialize)]
struct SseStreamItemPayload<'a, T> {
    id: u64,
    next: &'a T,
}

#[derive(Serialize)]
struct SseErrorPayload<'a> {
    id: u64,
    err: &'a RpcErrorBody,
}

fn format_message_event<T: Serialize>(id: u64, item: &T) -> Bytes {
    let payload = SseStreamItemPayload { id, next: item };
    let json =
        serde_json::to_string(&payload).unwrap_or_else(|_| r#"{"id":0,"next":null}"#.to_owned());
    Bytes::from(format!("event: message\ndata: {json}\n\n"))
}

fn format_error_event(id: u64, error: &RpcErrorBody) -> Bytes {
    let payload = SseErrorPayload { id, err: error };
    let json = serde_json::to_string(&payload).unwrap_or_else(|_| {
        r#"{"id":0,"err":{"code":"internal","message":"encode failure"}}"#.to_owned()
    });
    Bytes::from(format!("event: error\ndata: {json}\n\n"))
}

#[cfg(test)]
mod tests;
