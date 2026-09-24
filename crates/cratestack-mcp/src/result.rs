//! Tool outcomes as `CallToolResult`s.
//!
//! **Errors reveal no more than REST's error envelope** (ADR 0002 § Tools).
//! The `isError` content is exactly the JSON `CratestackError::into_response`
//! produces for REST — `{"code", "message", "details": null}` — so a 5xx
//! variant's operator detail (a SQL error, a store failure) stays in the
//! log, behind the same canned public message REST sends. The one thing
//! MCP says that REST's body does not is *which argument* failed to decode:
//! REST answers a malformed body with `CODEC_ERROR` "invalid request
//! payload", while an MCP agent is expected to correct itself and needs
//! the field (cratestack#1038). That names a field of the tool's own
//! published input schema, so it discloses nothing new.
//!
//! **Success** carries the output twice when it is an object, as
//! `structuredContent` and as a text block (2026-07-28 asks for both). Any
//! other output (a list, a scalar, an optional) has no `outputSchema`, so
//! it goes out as text only.

use cratestack_core::CratestackError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::Value;

use crate::table::ToolDescriptor;

pub(crate) fn success(descriptor: &ToolDescriptor, value: Value) -> CallToolResult {
    if descriptor.output_schema.is_some() && value.is_object() {
        CallToolResult::structured(value)
    } else {
        CallToolResult::success(vec![ContentBlock::text(value.to_string())])
    }
}

pub(crate) fn failure(descriptor: &ToolDescriptor, error: CratestackError) -> CallToolResult {
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "mcp_tool_call",
        cratestack_tool = descriptor.name,
        cratestack_error = error.code(),
        cratestack_detail = error.detail().unwrap_or(""),
        "cratestack mcp tool call failed",
    );
    let envelope = error.into_response();
    // `CratestackErrorResponse` is three plain fields; it cannot fail to
    // serialize, but a panic is not an acceptable way to find out.
    let text = serde_json::to_string(&envelope).unwrap_or_else(|_| {
        format!(r#"{{"code":"{}","message":"internal error"}}"#, envelope.code)
    });
    CallToolResult::error(vec![ContentBlock::text(text)])
}
