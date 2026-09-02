//! One codec-negotiated error envelope for middleware that runs *outside*
//! the generated router (cratestack#846).
//!
//! Every generated handler encodes its errors through
//! [`crate::encode_transport_result_with_status_for`] with the router's
//! own `C: HttpTransport` type parameter. The tower layers in this crate
//! ([`crate::ratelimit`], [`crate::idempotency`]) are wrapped *around*
//! that router by the consuming application, so they never see that type
//! parameter — and until this module existed they compensated by writing
//! `error.public_message()` into a `text/plain` body. A generated client
//! then decoded that body against the framework's error shape, failed,
//! and reported "unrecognized error body" instead of a typed code: a
//! dropped Redis connection in the rate-limit layer surfaced downstream
//! as `RPC call returned status 500 with an unrecognized error body`.
//!
//! The fix is *not* a second envelope. This module names the same two
//! codecs the generated routers are built with, negotiates the response
//! `Content-Type` through the same `Accept` machinery
//! (`transport::media_type`), and encodes the same two wire shapes the
//! rest of the stack emits — [`CratestackErrorResponse`] for REST and
//! [`RpcErrorBody`] for the RPC binding.
//!
//! ## Why a hardcoded codec pair is acceptable here
//!
//! A router *can* be built with a single codec (`JsonCodec` alone, say),
//! in which case this pair is wider than the router's. That mismatch is
//! unreachable for any real caller: the generated clients always send an
//! `Accept` header naming their own codec, so negotiation picks it. Only
//! a hand-rolled client sending no `Accept` at all could get CBOR here
//! and JSON from the handler behind it — and such a client, having
//! stated no preference, must accept either (RFC 9110 §12.5.1). The
//! alternative — threading a codec type parameter through
//! `RateLimitLayer`/`IdempotencyLayer` — would break every existing
//! construction site for that edge case.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{CratestackError, RouteTransportCapabilities};

use serde::Serialize;

use crate::codec::CodecSet;

use self::rpc_probe::is_rpc_path;
use crate::transport::{HttpTransport, select_transport_response_content_type};

/// The codec pair every generated `router()` is wired with by default —
/// CBOR primary, JSON secondary. Kept in lockstep with
/// `MIDDLEWARE_CAPABILITIES` below: `can_encode` is what actually filters
/// the advertised list down to what we can produce.
pub(crate) type MiddlewareCodec = CodecSet<CborCodec, JsonCodec>;

pub(crate) fn middleware_codec() -> MiddlewareCodec {
    CodecSet::new(CborCodec, JsonCodec)
}

/// Deliberately identical to the generated REST routes' capabilities and
/// to [`crate::rpc::RPC_BINDING_CAPABILITIES`] (both codecs, CBOR
/// default). A middleware error must not negotiate differently from the
/// handler it is sitting in front of, or a client that gets a 429 from
/// the layer would have to decode a different content type than the 200
/// it got a moment earlier.
const MIDDLEWARE_CAPABILITIES: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &["application/cbor", "application/json"],
    response_types: &["application/cbor", "application/json"],
    default_response_type: "application/cbor",
    supports_sequence_response: false,
};

/// Encode `error` as the error envelope this request's transport expects,
/// with the HTTP status from [`CratestackError::status_code`].
///
/// `path` selects the envelope, because the two bindings disagree on the
/// `code` vocabulary: the RPC binding emits gRPC-style lowercase codes
/// (`resource_exhausted`), REST emits screaming-snake (`TOO_MANY_REQUESTS`).
/// Both shapes are structurally identical `{code, message, details}` maps,
/// so getting the branch wrong degrades a code string rather than breaking
/// the decode — which is why [`is_rpc_path`] can afford to be a syntactic
/// test on the path instead of `MatchedPath` plumbing.
pub(crate) fn middleware_error_response(
    headers: &HeaderMap,
    path: &str,
    error: CratestackError,
) -> Response {
    let codec = middleware_codec();
    let status = error.status_code();
    if is_rpc_path(path) {
        let body = RpcErrorBody::from_cratestack(&error);
        encode_with_status(&codec, headers, status, &body)
    } else {
        encode_with_status(&codec, headers, status, &error.into_response())
    }
}

/// Encode `body` at `status`, negotiating the content type from `Accept`.
///
/// **The status is never rewritten by negotiation** — that is the whole
/// reason this does not simply call
/// [`crate::encode_transport_result_with_status_for`]. That helper turns
/// a negotiation failure into `fallback_error_response`, which replaces
/// the status with the *negotiation* error's own: a client sending
/// `Accept: text/html` turned a `429` into a `406 text/plain`, and a
/// malformed `Accept` turned it into a `400 text/plain` — caller-
/// triggerable, and a regression from the hand-built `text/plain` body
/// this module replaced, which was at least unconditionally a 429.
///
/// A middleware refusal is a *server-originated* response, and RFC 9110
/// §12.5.1 explicitly permits sending a representation the client did not
/// ask for rather than a 406 ("the server SHOULD send a 406 … or, if the
/// origin server is willing to disregard the Accept field, send a
/// representation anyway"). Disregarding it is strictly more useful here:
/// a caller who asked for `text/html` and gets CBOR at least learns it
/// was throttled, whereas a 406 discards the throttle entirely.
fn encode_with_status<T>(
    codec: &MiddlewareCodec,
    headers: &HeaderMap,
    status: StatusCode,
    body: &T,
) -> Response
where
    T: Serialize + ?Sized,
{
    let content_type =
        select_transport_response_content_type(codec, headers, &MIDDLEWARE_CAPABILITIES)
            .unwrap_or(MIDDLEWARE_CAPABILITIES.default_response_type);
    codec
        .encode_response(content_type, status, body)
        .unwrap_or_else(|_| last_resort_response(status))
}

/// Unreachable in practice — the two bodies are plain structs of `String`
/// and `Option`, and the content type is one this codec pair advertises.
/// It exists so the encode path has no `unwrap`, and it keeps the
/// original status for exactly the reason above.
fn last_resort_response(status: StatusCode) -> Response {
    let mut response = Response::new(axum::body::Body::from(status.as_str().to_owned()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}


mod rpc_probe;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_negotiation;
