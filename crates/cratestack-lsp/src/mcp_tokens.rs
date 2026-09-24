//! Semantic-token span for the MCP surface (ADR 0002, cratestack#1036).
//!
//! `@@mcp(...)` used to be a raw model attribute and got the decorator token
//! every `@@...` gets from `semantic_tokens::collect_attributes`. The parser
//! now moves it out of `Model.attributes` into the typed `Model.mcp`, so
//! that walk no longer sees it; this restores the token from the typed span.
//! `@mcp(...)` on a procedure needs nothing: procedure attributes have never
//! been tokenised.

use cratestack_core::{Model, SourceSpan};

/// The `@@mcp` head of the model's MCP attribute, if it has one — the same
/// head-only span `collect_attributes` gives every other attribute.
pub(crate) fn decorator(model: &Model) -> Option<SourceSpan> {
    model.mcp.as_ref().map(|mcp| SourceSpan {
        start: mcp.span.start,
        end: mcp.span.start + "@@mcp".len(),
        line: mcp.span.line,
    })
}
