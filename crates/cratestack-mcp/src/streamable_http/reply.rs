//! The responses the guard gives before `rmcp` is reached: 403, 405, 401,
//! 413 and a provider's 5xx.
//!
//! Plain HTTP, not JSON-RPC. None of these requests reached MCP handling,
//! and a 401 must be readable by a client that knows only RFC 6750. The
//! body is REST's error envelope (`{"code","message","details":null}`), so
//! an operator sees one shape across REST, RPC and MCP, and a provider's
//! 5xx detail stays in the log as it does on REST.

use std::convert::Infallible;

use bytes::Bytes;
use cratestack_core::CratestackError;
use http::header::{ALLOW, CONTENT_TYPE, WWW_AUTHENTICATE};
use http::{HeaderValue, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};

/// The response type `rmcp`'s service answers with, so the guard's own
/// replies and `rmcp`'s share one type.
pub(crate) type Reply = Response<BoxBody<Bytes, Infallible>>;

fn envelope(status: StatusCode, code: &str, message: &str) -> Reply {
    let body = serde_json::json!({ "code": code, "message": message, "details": null });
    let mut reply = Response::new(Full::new(Bytes::from(body.to_string())).boxed());
    *reply.status_mut() = status;
    reply
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    reply
}

pub(crate) fn from_error(error: CratestackError) -> Reply {
    let status = error.status_code();
    let envelope_body = error.into_response();
    envelope(status, &envelope_body.code, &envelope_body.message)
}

pub(crate) fn forbidden_origin() -> Reply {
    envelope(
        StatusCode::FORBIDDEN,
        "FORBIDDEN",
        "Origin header is not allowed",
    )
}

/// MCP 2026-07-28 has one endpoint, `POST`; `GET` (the old server-sent
/// stream) and `DELETE` (session end) answer 405.
pub(crate) fn method_not_allowed() -> Reply {
    let mut reply = envelope(
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
        "the MCP endpoint accepts POST only",
    );
    reply
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static("POST"));
    reply
}

pub(crate) fn payload_too_large() -> Reply {
    envelope(
        StatusCode::PAYLOAD_TOO_LARGE,
        "PAYLOAD_TOO_LARGE",
        "request body too large",
    )
}

pub(crate) fn bad_body() -> Reply {
    envelope(
        StatusCode::BAD_REQUEST,
        "BAD_REQUEST",
        "request body could not be read",
    )
}

/// 401 or 403 with the RFC 6750 challenge naming the metadata document.
pub(crate) fn challenge(status: StatusCode, header: HeaderValue) -> Reply {
    let (code, message) = if status == StatusCode::FORBIDDEN {
        ("FORBIDDEN", "the access token does not grant this request")
    } else {
        ("UNAUTHORIZED", "a valid bearer access token is required")
    };
    let mut reply = envelope(status, code, message);
    reply.headers_mut().insert(WWW_AUTHENTICATE, header);
    reply
}
