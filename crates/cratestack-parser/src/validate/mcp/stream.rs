//! ADR 0002 Q8 (decided 2026-09-24, cratestack#1038): a `@stream` procedure
//! cannot be an MCP tool.
//!
//! An MCP tool result is a single response. A `@stream` procedure's
//! registry method returns a `Stream` that the HTTP transports encode one
//! item at a time; there is no faithful single-result rendering of that,
//! and buffering it would quietly turn an unbounded stream into an
//! unbounded allocation. So the combination is refused where it is
//! declared, which also puts it in front of the LSP, rather than left for
//! the MCP codegen to trip over.

use cratestack_core::Schema;

use super::tools;
use crate::diagnostics::{SchemaError, span_error};

pub(super) fn tools_are_not_streams(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (procedure, tool) in tools(schema) {
        if procedure
            .attributes
            .iter()
            .any(|attribute| attribute.raw == "@stream")
        {
            errors.push(span_error(
                format!(
                    "`@mcp(tool)` on procedure `{}` exposes a `@stream` procedure: an MCP tool \
                     returns a single result, so a streaming procedure cannot be a tool (ADR \
                     0002 Q8). Remove `@mcp(tool)`, or expose a non-streaming procedure instead",
                    procedure.name
                ),
                tool.span,
            ));
        }
    }
}
