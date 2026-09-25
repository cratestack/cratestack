//! Resource failures as JSON-RPC errors.
//!
//! A resource read has no `isError` result the way a tool call does: every
//! failure is a protocol error. Three rules decide which one.
//!
//! 1. **Not found and not visible are one error** (ADR 0002 security
//!    requirement 12). [`not_found`] takes no argument at all, so no detail
//!    about the row can reach the message. `NOT_FOUND` and
//!    `FORBIDDEN` from the read path land there too: the ORM filters hidden
//!    rows in SQL and answers `None` rather than either, but if some future
//!    path did say "forbidden" for a row it found, answering differently
//!    from a missing row would be exactly the oracle this rule forbids.
//! 2. **The caller's own mistakes are `-32602`**, with the message
//!    restating the caller's input (a bad `limit`, a tampered cursor).
//! 3. **Everything else is `-32603`** carrying REST's public error envelope
//!    — its canned message and its `code` in `data` — so a 5xx detail stays
//!    in the log, as on REST and for tools (`src/result.rs`). A throttled
//!    read is `-32603` with `data.code = "TOO_MANY_REQUESTS"`; MCP has no
//!    code of its own for it.

use cratestack_core::CratestackError;
use rmcp::ErrorData;
use serde_json::json;

/// The one answer for an unknown resource, a missing row and a hidden row:
/// a constant, byte for byte. It does not even echo the URI, so "the same
/// error" needs no normalizing to check, and nothing a later edit adds to
/// the message can differ between the cases.
pub(crate) fn not_found() -> ErrorData {
    ErrorData::invalid_params(NOT_FOUND, None)
}

const NOT_FOUND: &str = "resource not found";

pub(crate) fn invalid(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

pub(crate) fn from_cratestack(uri: &str, error: CratestackError) -> ErrorData {
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "mcp_resource_read",
        // The operator's log may name the URI; the caller's error may not.
        cratestack_resource = uri,
        cratestack_error = error.code(),
        cratestack_detail = error.detail().unwrap_or(""),
        "cratestack mcp resource read failed",
    );
    match error {
        CratestackError::NotFound(_) | CratestackError::Forbidden(_) => not_found(),
        CratestackError::Validation(_) | CratestackError::BadRequest(_) => {
            let envelope = error.into_response();
            ErrorData::invalid_params(envelope.message, Some(json!({ "code": envelope.code })))
        }
        other => {
            let envelope = other.into_response();
            ErrorData::internal_error(envelope.message, Some(json!({ "code": envelope.code })))
        }
    }
}
