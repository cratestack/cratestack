//! The layer's own refusals, which go out **unsigned** (decision D4): they
//! are decided before, or instead of, a verified request, so there is no
//! request a signature could honestly bind them to. Each is the
//! transport's usual error shape (`RpcErrorBody` on `/rpc/...`,
//! `CratestackErrorResponse` otherwise), negotiated from the request's own
//! headers like every other middleware error.

use axum::response::Response;
use cratestack_core::{CratestackError, UNAUTHENTICATED};
use http::{HeaderMap, HeaderValue, Method, StatusCode};

use super::request::BufferError;
use crate::middleware_error::{middleware_error_response, middleware_error_response_with_status};

/// A signed request that did not verify, an unsigned one under `Required`,
/// or a signer the principal mapper refused: one coarse `401`, whatever the
/// envelope reported (ADR 0006 §10). Unsigned, so a replayed request cannot
/// earn a signed "401" for a request that already ran.
pub(super) fn unauthenticated(headers: &HeaderMap, path: &str) -> Response {
    middleware_error_response(
        headers,
        path,
        CratestackError::Unauthorized(UNAUTHENTICATED.to_owned()),
    )
}

/// A COSE body the layer will not open (policy `Off`, or no generated op to
/// bind it to). Refused rather than forwarded, so nothing behind the layer
/// ever reads unverified COSE bytes as a plain body.
pub(super) fn unsupported_envelope(headers: &HeaderMap, path: &str) -> Response {
    middleware_error_response(
        headers,
        path,
        CratestackError::UnsupportedMediaType(
            "a signed body is not accepted by this route".to_owned(),
        ),
    )
}

/// A body the layer could not buffer: over its own cap (`413`; it buffers
/// before the router's `DefaultBodyLimit` runs, so it needs one of its
/// own), or a body stream that failed (`400`).
pub(super) fn unbuffered(headers: &HeaderMap, path: &str, error: BufferError) -> Response {
    match error {
        BufferError::TooLarge => middleware_error_response_with_status(
            headers,
            path,
            StatusCode::PAYLOAD_TOO_LARGE,
            CratestackError::BadRequest(
                "request body exceeds the envelope layer's limit".to_owned(),
            ),
        ),
        BufferError::Unreadable => bad_request(
            headers,
            path,
            CratestackError::BadRequest("request body could not be read".to_owned()),
        ),
    }
}

/// A request the layer cannot bind as sent (a bound header sent twice, a
/// `/rpc/batch` body whose frames it cannot read). `error` is a
/// `BadRequest` whose message is public.
pub(super) fn bad_request(headers: &HeaderMap, path: &str, error: CratestackError) -> Response {
    middleware_error_response_with_status(headers, path, StatusCode::BAD_REQUEST, error)
}

/// A generated path, a method the schema did not generate for it, under a
/// `Required` `unresolved_mode` (decision S2): the `405` the router would
/// have given, answered here so no hand-written handler for that method
/// runs unsigned.
pub(super) fn method_not_allowed(headers: &HeaderMap, path: &str, allow: &[Method]) -> Response {
    let mut response = middleware_error_response_with_status(
        headers,
        path,
        StatusCode::METHOD_NOT_ALLOWED,
        CratestackError::BadRequest("method not allowed".to_owned()),
    );
    let allow: Vec<&str> = allow.iter().map(Method::as_str).collect();
    if let Ok(value) = HeaderValue::from_str(&allow.join(", ")) {
        response.headers_mut().insert(http::header::ALLOW, value);
    }
    response
}

/// A backend or local failure (key resolver, nonce store, signer, a
/// misconfigured layer) before a response could be sealed. The detail is
/// logged here and never sent.
pub(super) fn internal(
    headers: &HeaderMap,
    path: &str,
    stage: &'static str,
    error: &CratestackError,
) -> Response {
    tracing::error!(
        target: "cratestack",
        cratestack_operation = "envelope",
        cratestack_stage = stage,
        cratestack_error = error.code(),
        cratestack_detail = error.detail().unwrap_or(""),
        "envelope layer failed",
    );
    plain_internal(headers, path)
}

/// The public half of [`internal`], for a caller that logged already.
pub(super) fn plain_internal(headers: &HeaderMap, path: &str) -> Response {
    middleware_error_response(headers, path, CratestackError::Internal(String::new()))
}
