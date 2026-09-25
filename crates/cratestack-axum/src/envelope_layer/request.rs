//! Reading a request into the layer, and rewriting it for the router.

use axum::body::Body;
use axum::extract::{FromRequestParts, MatchedPath, RawPathParams};
use bytes::Bytes;
use http::header::{ACCEPT, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use http::request::Parts;
use http::{HeaderMap, HeaderValue};

use super::media;
use super::mode::EnvelopeMode;

/// The route template and decoded path parameters axum matched, if any.
///
/// A parameter that is not valid UTF-8 makes the extractor fail; the
/// parameters are then empty, which fails closed: a signed request's
/// binding no longer matches the client's, and an unsigned one gets no
/// further than it would have.
pub(super) async fn matched_route(parts: &mut Parts) -> (Option<String>, Vec<(String, String)>) {
    let matched = parts
        .extensions
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned());
    let params = match RawPathParams::from_request_parts(parts, &()).await {
        Ok(params) => params
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        Err(_) => Vec::new(),
    };
    (matched, params)
}

/// The body, up to `limit` bytes. `None` when it is longer or unreadable.
pub(super) async fn buffer(body: Body, limit: usize) -> Option<Bytes> {
    axum::body::to_bytes(body, limit).await.ok()
}

/// Make an opened request look like the plain CBOR request it wraps: the
/// payload's length and type, and nothing left that would make a layer or
/// handler behind this one re-interpret it (a stale length, a chunked or
/// compressed encoding that no longer applies).
pub(super) fn rewrite_opened(headers: &mut HeaderMap, payload: &Bytes) {
    headers.remove(CONTENT_TYPE);
    headers.remove(CONTENT_ENCODING);
    headers.remove(TRANSFER_ENCODING);
    headers.insert(CONTENT_LENGTH, HeaderValue::from(payload.len()));
    // A bodiless request (`GET`, `DELETE`) seals an empty payload (D3); it
    // reaches the router bodiless, as the unsigned one would have.
    if !payload.is_empty() {
        headers.insert(CONTENT_TYPE, media::cbor_header_value());
    }
}

/// Ask the router for the one representation the response binding names.
///
/// Under `Required` always `application/cbor`, so a `@stream` op answers
/// with one buffered array and every response can be sealed. Under
/// `Optional` a client that asks for a stream (`application/cbor-seq`,
/// `text/event-stream`) keeps its `Accept`: streams cannot be sealed until
/// ADR 0006 P1, and `Optional` lets them go plain.
pub(super) fn rewrite_accept(headers: &mut HeaderMap, mode: EnvelopeMode) {
    if mode == EnvelopeMode::Optional && media::accept_names_stream(headers) {
        return;
    }
    headers.remove(ACCEPT);
    headers.insert(ACCEPT, media::cbor_header_value());
}
