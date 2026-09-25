//! The layer's own refusals, which go out **unsigned** (decision D4): they
//! are decided before, or instead of, a verified request, so there is no
//! request a signature could honestly bind them to. Each is the
//! transport's usual error shape (`RpcErrorBody` on `/rpc/...`,
//! `CratestackErrorResponse` otherwise), negotiated from the request's own
//! headers like every other middleware error.

use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_cose::UNAUTHENTICATED;
use http::{HeaderMap, StatusCode};

use crate::middleware_error::{middleware_error_response, middleware_error_response_with_status};

/// A signed request that did not verify, or an unsigned one under
/// `Required`: one coarse `401`, whatever the envelope reported (ADR 0006
/// §10). Unsigned, so a replayed request cannot earn a signed "401" for a
/// request that already ran.
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

/// A body over the layer's own cap. The layer buffers before the router's
/// `DefaultBodyLimit` runs, so it needs one of its own.
pub(super) fn too_large(headers: &HeaderMap, path: &str) -> Response {
    middleware_error_response_with_status(
        headers,
        path,
        StatusCode::PAYLOAD_TOO_LARGE,
        CratestackError::BadRequest("request body exceeds the envelope layer's limit".to_owned()),
    )
}

/// A backend or local failure (key resolver, nonce store, signer) before a
/// response could be sealed. The detail is logged here and never sent.
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
    middleware_error_response(headers, path, CratestackError::Internal(String::new()))
}
