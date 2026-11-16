//! A signed subscription open that streams unsigned frames (ADR 0006,
//! cratestack#1378): the open is verified like a unary call, the stream that
//! answers carries plain frames, and the response says so.
//!
//! Only a subscription the schema marks `@frames(unsigned)` may be opened
//! this way ([`lookup`]); every other signed subscription open is refused
//! with a sealed `406`, and the frames of a marked one are never sealed: a
//! client treats them as hints.

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use cratestack_core::{CratestackError, FRAMES_HEADER, PAYLOAD_TYPE_HEADER, VerifiedSigner};
use http::header::{ACCEPT, CACHE_CONTROL};
use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use tokio::time::Instant;

use super::layer::Config;
use super::media;
use super::resolver::ResolvedRoute;
use super::seal::Sealer;
use super::service::{Inner, call};
use super::signed_principal::admit;
use crate::rpc::SignedStreamDeadline;
use crate::transport::{CBOR_SEQUENCE_CONTENT_TYPE, StreamedResponseMarker};

const SSE: &str = "text/event-stream";

/// What the frames table says about a signed request's route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Frames {
    /// Not a subscription the table could mark: decided as any call.
    NotSubscription,
    /// An RPC subscription with no `@frames` row: refused before its handler.
    Unmarked,
    /// A subscription marked `@frames(unsigned)`.
    Marked,
}

/// Decide `route` against `config.subscriptions`. The key is the route's
/// contract key, else its op. On RPC (`is_subscription()`) the bare key
/// matches, and a unary call to the same op id is not an open. On REST the
/// row is `"<METHOD> <key>"`, matched on the method as sent, so a `HEAD`
/// never matches. Not `find_contract`, which matches a bare key for any
/// method and maps `HEAD` to `GET`.
pub(super) fn lookup(config: &Config, method: &Method, route: &ResolvedRoute) -> Frames {
    let key = route.contract_key().unwrap_or_else(|| route.op());
    let rows = config.subscriptions;
    if route.is_subscription() {
        return match rows.iter().any(|(row, _)| *row == key) {
            true => Frames::Marked,
            false => Frames::Unmarked,
        };
    }
    let qualified = key
        .split_once(' ')
        .is_some_and(|(prefix, _)| prefix == method.as_str());
    let found = rows.iter().any(|(row, _)| match qualified {
        true => *row == key,
        false => {
            row.strip_prefix(method.as_str())
                .and_then(|rest| rest.strip_prefix(' '))
                == Some(key)
        }
    });
    match found {
        true => Frames::Marked,
        false => Frames::NotSubscription,
    }
}

/// The request's `Accept`, as sent: `None` when it is absent, unreadable or
/// repeated (two headers are ambiguous, so none is honoured).
pub(super) fn single_accept(headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all(ACCEPT).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return None;
    };
    value.to_str().ok().map(str::to_owned)
}

/// The stream type the client asked for: the first entry that is literally
/// `application/cbor-seq` or `text/event-stream`. `*/*` asks for nothing, so
/// a client that wants an unsigned stream says so.
pub(super) fn requested_binding(accept: Option<&str>) -> Option<&'static str> {
    accept?
        .split(',')
        .map(str::trim)
        .find_map(|entry| match entry {
            CBOR_SEQUENCE_CONTENT_TYPE => Some(CBOR_SEQUENCE_CONTENT_TYPE),
            SSE => Some(SSE),
            _ => None,
        })
}

/// Serve a verified open of a marked subscription.
pub(super) async fn handle<S: Inner>(
    config: &Config,
    inner: S,
    mut parts: Parts,
    payload: Bytes,
    signer: VerifiedSigner,
    sealer: Sealer,
    accept: Option<String>,
) -> Response {
    if !matches!(sealer.inputs.method, Method::GET | Method::POST) {
        return refuse(&sealer, "a signed subscription is opened with GET or POST").await;
    }
    let Some(stream_type) = requested_binding(accept.as_deref()) else {
        let need =
            "a signed subscription open names Accept: application/cbor-seq or text/event-stream";
        return refuse(&sealer, need).await;
    };
    if let Some(response) = admit(config, &sealer, signer, &mut parts.extensions).await {
        return response;
    }
    match Instant::now().checked_add(config.max_signed_stream_life) {
        Some(at) => {
            parts.extensions.insert(SignedStreamDeadline(at));
        }
        None => tracing::warn!(
            target: "cratestack",
            cratestack_operation = "envelope",
            "max_signed_stream_life overflows the clock: no stream deadline applied",
        ),
    }
    parts
        .headers
        .insert(ACCEPT, HeaderValue::from_static(stream_type));
    let response = call(inner, Request::from_parts(parts, Body::from(payload))).await;
    let streamed = response.status().is_success()
        && response
            .extensions()
            .get::<StreamedResponseMarker>()
            .is_some()
        && media::response_media_type(response.headers())
            .is_some_and(|media_type| media_type.eq_ignore_ascii_case(stream_type));
    if !streamed {
        return sealer.finish(response).await;
    }
    let mut response = response;
    let headers = response.headers_mut();
    headers.remove(PAYLOAD_TYPE_HEADER);
    headers.insert(FRAMES_HEADER, HeaderValue::from_static("unsigned"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn refuse(sealer: &Sealer, message: &str) -> Response {
    let error = CratestackError::NotAcceptable(message.to_owned());
    sealer.seal_error(StatusCode::NOT_ACCEPTABLE, error).await
}
