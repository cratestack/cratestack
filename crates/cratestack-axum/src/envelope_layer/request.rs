//! Reading a request into the layer, and rewriting it for the router.

use axum::body::{Body, HttpBody};
use axum::extract::{FromRequestParts, MatchedPath, RawPathParams};
use bytes::Bytes;
use futures_util::StreamExt;
use http::header::{ACCEPT, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use http::request::Parts;
use http::{HeaderMap, HeaderValue};

use super::media;

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

/// Why a body could not be buffered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BufferError {
    /// Longer than the layer's cap: `413`.
    TooLarge,
    /// The body stream failed (a reset connection, a bad chunk): `400`,
    /// not `413`, which would blame the client's size for a transport
    /// fault (decision on the security review's nits).
    Unreadable,
}

/// The body, up to `limit` bytes. A body that arrives in one chunk (every
/// in-memory body, and most small ones off the wire) is returned without a
/// copy.
pub(super) async fn buffer(body: Body, limit: usize) -> Result<Bytes, BufferError> {
    let mut stream = body.into_data_stream();
    let mut first: Option<Bytes> = None;
    let mut joined: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BufferError::Unreadable)?;
        let held = first.as_ref().map_or(0, Bytes::len) + joined.len();
        if held.saturating_add(chunk.len()) > limit {
            return Err(BufferError::TooLarge);
        }
        match first.take() {
            None if joined.is_empty() => first = Some(chunk),
            None => joined.extend_from_slice(&chunk),
            Some(head) => {
                joined.extend_from_slice(&head);
                joined.extend_from_slice(&chunk);
            }
        }
    }
    Ok(first.unwrap_or_else(|| Bytes::from(joined)))
}

/// Whether the request certainly has no body: a CORS preflight, which
/// passes through unsigned whatever the policy (decision on the security
/// review's nits). Read from the body's own size hint, which the server
/// knows from the framing (no `Content-Length` and no `Transfer-Encoding`
/// on HTTP/1.1, `END_STREAM` on the headers in HTTP/2), not from headers a
/// client could leave out while still sending a body.
pub(super) fn is_bodiless(body: &Body) -> bool {
    HttpBody::size_hint(body).exact() == Some(0)
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
/// For a signed request (under `Required` or `Optional`, decision S3) and
/// under `Required`, always `application/cbor`: a `@stream` op then answers
/// with one buffered array, and every response can be sealed. A client
/// asking a signed request for a stream does not get a plain one. For an
/// unsigned, nonce-bound request under `Optional`, a client that asks for a
/// stream (`application/cbor-seq`, `text/event-stream`) keeps its `Accept`:
/// streams cannot be sealed until ADR 0006 P1, and there it may go plain.
pub(super) fn rewrite_accept(headers: &mut HeaderMap, strict: bool) {
    if !strict && media::accept_names_stream(headers) {
        return;
    }
    headers.remove(ACCEPT);
    headers.insert(ACCEPT, media::cbor_header_value());
}
