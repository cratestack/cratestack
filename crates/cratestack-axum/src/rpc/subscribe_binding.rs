//! The one path every subscribe route shares: negotiate the wire binding
//! from `Accept`, then open the stream and encode the response (cratestack#1001).
//!
//! The four generated subscribe handlers (RPC `GET` and `POST`, REST
//! procedure, REST model) call [`negotiate_subscribe_binding`] before they
//! authenticate, so a wrong `Accept` is refused before any credential work or
//! feed registration, and [`serve_subscription`] once they hold a context. A
//! second binding is added here, not in each handler.

use std::future::Future;

use axum::http::{HeaderMap, header};
use axum::response::Response;
use cratestack_core::{CratestackError, SubscriptionStream};
use serde::Serialize;

use super::sse::SSE_CONTENT_TYPE;
use super::sse_subscription::encode_subscription_sse_response;
use super::subscribe_error::encode_subscribe_error;

/// The wire binding a subscribe request asked for.
///
/// `CborSeq` arrives in cratestack#1001 PR 2; until then SSE is the only
/// binding, and `Accept` negotiation answers it or refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscribeBinding {
    /// `text/event-stream`.
    Sse,
}

/// Choose the binding from the request's `Accept` header.
///
/// `GET /rpc/subscribe/{op_id}` has no upgrade handshake to negotiate a
/// subprotocol like WS does: the client states its intent with a plain
/// `Accept`. Anything that does not ask for SSE (`text/event-stream` or
/// `*/*`, matched literally per comma-separated entry) is refused up front,
/// before any event-bus subscription is registered.
pub fn negotiate_subscribe_binding(
    headers: &HeaderMap,
) -> Result<SubscribeBinding, CratestackError> {
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
        Ok(SubscribeBinding::Sse)
    } else {
        Err(CratestackError::NotAcceptable(format!(
            "subscription endpoint requires Accept: {SSE_CONTENT_TYPE}, got {accept}"
        )))
    }
}

/// The pre-#1001 `Accept` check, kept so existing callers still compile.
///
/// Same signature and same results as before; it now defers to
/// [`negotiate_subscribe_binding`] and drops the binding it chose.
#[deprecated(since = "0.20.3", note = "use negotiate_subscribe_binding")]
pub fn validate_subscribe_accept_header(headers: &HeaderMap) -> Result<(), CratestackError> {
    negotiate_subscribe_binding(headers).map(|_| ())
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
        },
        Err(error) => encode_subscribe_error(&error),
    }
}

#[cfg(test)]
mod tests;
