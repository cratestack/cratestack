//! The one path every subscribe route shares: negotiate the wire binding
//! from `Accept`, then open the stream and encode the response (cratestack#1001).
//!
//! The four generated subscribe handlers (RPC `GET` and `POST`, REST
//! procedure, REST model) call [`negotiate_subscribe_binding`] before they
//! authenticate, so a wrong `Accept` is refused before any credential work or
//! feed registration, and [`serve_subscription`] once they hold a context. A
//! new binding is added here, not in each handler: [`SubscribeBinding::codec`]
//! is what each handler passes the dispatcher, so it follows the binding.

use std::future::Future;

use axum::http::{HeaderMap, header};
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CratestackError, SubscriptionStream};
use serde::Serialize;

use super::cbor_seq_subscription::{CBOR_SEQ_CONTENT_TYPE, encode_subscription_cbor_seq_response};
use super::sse::SSE_CONTENT_TYPE;
use super::sse_subscription::encode_subscription_sse_response;
use super::subscribe_codec::SubscribeCodec;
use super::subscribe_error::encode_subscribe_error;

/// The wire binding a subscribe request asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscribeBinding {
    /// `text/event-stream`: JSON events.
    Sse,
    /// `application/cbor-seq`: one CBOR `ServerFrame` per item, the framing
    /// WebTransport uses (design §6a.3).
    CborSeq,
}

impl SubscribeBinding {
    /// The codec a dispatcher encodes this binding's items with, so a `Uuid`
    /// is a byte string on `CborSeq` and text on `Sse`.
    pub fn codec(self) -> SubscribeCodec {
        match self {
            Self::Sse => SubscribeCodec::Json(JsonCodec),
            Self::CborSeq => SubscribeCodec::Cbor(CborCodec),
        }
    }
}

/// Choose the binding from the request's `Accept` header.
///
/// `GET /rpc/subscribe/{op_id}` has no upgrade handshake to negotiate a
/// subprotocol like WS does: the client states its intent with a plain
/// `Accept`. Entries are matched literally and the first one this server
/// speaks wins, so a client lists its preference in order:
/// `application/cbor-seq` is [`SubscribeBinding::CborSeq`];
/// `text/event-stream` and `*/*` are [`SubscribeBinding::Sse`]. Anything else
/// is refused up front, before any event-bus subscription is registered.
pub fn negotiate_subscribe_binding(
    headers: &HeaderMap,
) -> Result<SubscribeBinding, CratestackError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Err(CratestackError::NotAcceptable(format!(
            "subscription endpoint requires {NEED}"
        )));
    };
    let accept = accept
        .to_str()
        .map_err(|error| CratestackError::BadRequest(format!("invalid Accept header: {error}")))?;
    accept
        .split(',')
        .map(str::trim)
        .find_map(|value| match value {
            CBOR_SEQ_CONTENT_TYPE => Some(SubscribeBinding::CborSeq),
            SSE_CONTENT_TYPE | "*/*" => Some(SubscribeBinding::Sse),
            _ => None,
        })
        .ok_or_else(|| {
            CratestackError::NotAcceptable(format!(
                "subscription endpoint requires {NEED}, got {accept}"
            ))
        })
}

const NEED: &str = "Accept: text/event-stream or application/cbor-seq";

/// The pre-#1001 `Accept` check, kept so existing callers still compile.
///
/// Same signature and same results as before: it answers whether the request
/// may be served as SSE, and knows nothing of `application/cbor-seq`. A caller
/// that serves SSE only keeps working unchanged; one that wants the new
/// binding uses [`negotiate_subscribe_binding`].
#[deprecated(since = "0.20.3", note = "use negotiate_subscribe_binding")]
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

/// Await `open` and encode its stream in `binding`'s framing; an open
/// failure is the subscribe endpoint's JSON refusal with its own status.
pub async fn serve_subscription<T, F>(binding: SubscribeBinding, open: F) -> Response
where
    T: Serialize + Send + 'static,
    F: Future<Output = Result<SubscriptionStream<T>, CratestackError>>,
{
    match open.await {
        Ok(stream) => match binding {
            SubscribeBinding::Sse => encode_subscription_sse_response(stream),
            SubscribeBinding::CborSeq => encode_subscription_cbor_seq_response(stream),
        },
        Err(error) => encode_subscribe_error(&error),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cbor_seq;
