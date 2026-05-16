//! Response helpers used by the middleware service: replay, in-flight,
//! error.

use axum::body::Body;
use axum::response::Response;
use cratestack_core::CoolError;
use http::{StatusCode, header};

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
pub(super) fn in_flight_response() -> Response {
    let mut response = Response::new(Body::from(
        "another request with this Idempotency-Key is still in flight",
    ));
    *response.status_mut() = StatusCode::CONFLICT;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, http::HeaderValue::from_static("1"));
    response
}

pub(super) fn error_response(error: CoolError) -> Response {
    let status = error.status_code();
    let mut response = Response::new(Body::from(error.public_message().into_owned()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
