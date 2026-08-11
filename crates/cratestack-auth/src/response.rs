//! Minimal JSON error-response helper for this crate's `axum`-gated surface
//! (`middleware::require_signed_request`, and the `FromRequestParts` impls
//! on [`crate::CurrentPrincipal`]/[`crate::AuthenticatedPrincipal`]).
//!
//! Deliberately does not do content negotiation (JSON vs CBOR vs COSE) —
//! the downstream crate this was absorbed from carried a bespoke
//! multi-format response encoder for exactly that, which was
//! application-specific and out of scope for this framework crate. This
//! helper emits [`cratestack_core::CoolErrorResponse`]-shaped JSON, the same
//! REST error envelope `cratestack-axum`'s own generated handlers emit, so
//! a caller who wants CBOR/content-negotiated error bodies can layer their
//! own conversion on top rather than this crate reimplementing one.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use cratestack_core::CoolErrorResponse;

pub(crate) fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = CoolErrorResponse {
        code: code.to_string(),
        message: message.to_string(),
        details: None,
    };
    (status, axum::Json(body)).into_response()
}
