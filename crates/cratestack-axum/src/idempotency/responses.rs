//! Response helpers used by the middleware service: replay, in-flight,
//! error.
//!
//! Every *error-shaped* response here goes through
//! [`crate::middleware_error::middleware_error_response`], the same
//! codec-negotiated envelope the rate-limit layer emits (cratestack#846).
//! Before that, these were hand-built `text/plain` bodies, so a generated
//! client hitting an idempotency-key conflict — a routine, expected
//! outcome, not an edge case — got "unrecognized error body" instead of
//! the `CONFLICT` code it is supposed to branch on. Unlike the rate-limit
//! layer, idempotency gains no fail-open policy: a failed idempotency
//! store must keep failing the request, since the whole point is refusing
//! to execute a mutation twice.

use axum::body::Body;
use axum::response::Response;
use cratestack_core::CratestackError;
use http::{HeaderMap, StatusCode, header};

use crate::middleware_error::middleware_error_response;

use super::headers::decode_headers;
use super::record::IdempotencyRecord;

pub(super) fn replay_response(record: &IdempotencyRecord) -> Response {
    let mut response = Response::new(Body::from(record.response_body.clone()));
    *response.status_mut() = StatusCode::from_u16(record.response_status).unwrap_or(StatusCode::OK);
    // Restore every header the handler originally set (Location,
    // ETag, Cache-Control, Content-Type, Set-Cookie, …). The
    // replay marker is appended after so downstream clients can
    // still distinguish a replay from a live execution.
    let restored = decode_headers(&record.response_headers);
    let response_headers = response.headers_mut();
    for (name, value) in restored.iter() {
        response_headers.append(name.clone(), value.clone());
    }
    response_headers.append(
        http::HeaderName::from_static("idempotency-replayed"),
        http::HeaderValue::from_static("true"),
    );
    response
}

/// 409 Conflict response when another request holds the reservation.
/// Banks that need a deterministic outcome should retry; `Retry-After: 1`
/// is conservative so the caller doesn't busy-loop the server.
pub(super) fn in_flight_response(headers: &HeaderMap, path: &str) -> Response {
    let mut response = middleware_error_response(
        headers,
        path,
        CratestackError::Conflict(
            "another request with this Idempotency-Key is still in flight".to_owned(),
        ),
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, http::HeaderValue::from_static("1"));
    response
}
