//! The top-level `mcp { expose = [tools, resources] }` block (ADR 0002
//! § Schema surface).
//!
//! Before cratestack#1036 this went through `parse_simple_config_block` and
//! became opaque text lines in `Schema.config_blocks` that nothing read, so
//! `mcp { anything at all }` parsed. The body is now `key = value` like every
//! other config block — the maintainer's 2026-09-24 choice, which is also
//! what tree-sitter-cstack's `config_body` already accepts — and `expose` is
//! its only key. Everything else is an error.

use cratestack_core::McpConfig;

use super::expose::{ExposeList, old_line_form_hint, parse_expose_list};
use crate::diagnostics::{SchemaError, span_error};
use crate::line_helpers::{Line, span_from_lines, trimmed_span};

pub(crate) fn parse_mcp_block(
    lines: &[Line<'_>],
    start: usize,
    docs: Vec<String>,
    previous: Option<&McpConfig>,
) -> Result<(McpConfig, usize), SchemaError> {
    let header = &lines[start];
    let mut expose: Option<ExposeList> = None;
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = &lines[cursor];
        cursor += 1;
        if line.trimmed == "}" {
            let span = span_from_lines(header, line);
            if let Some(previous) = previous {
                return Err(span_error(
                    format!(
                        "duplicate `mcp {{ }}` block (the first is on line {}); a schema has \
                         one MCP configuration",
                        previous.span.line
                    ),
                    span,
                ));
            }
            let Some(expose) = expose else {
                return Err(span_error(
                    "`mcp { }` block has no `expose` key, so it would turn MCP on for no \
                     declaration: write `expose = [tools]`, `expose = [resources]` or \
                     `expose = [tools, resources]`",
                    span,
                ));
            };
            let config = McpConfig {
                docs,
                expose_tools: expose.tools,
                expose_resources: expose.resources,
                span,
            };
            return Ok((config, cursor));
        }
        if line.trimmed.is_empty() || line.trimmed.starts_with("//") {
            continue;
        }
        let Some((key, value)) = line.trimmed.split_once('=') else {
            let message = old_line_form_hint(line.trimmed).unwrap_or_else(|| {
                format!(
                    "unsupported `mcp` block entry `{}` (the block takes `expose = [...]`)",
                    line.trimmed
                )
            });
            return Err(entry_error(line, &message));
        };
        match key.trim() {
            "expose" if expose.is_some() => {
                return Err(entry_error(line, "`expose` is set more than once"));
            }
            "expose" => {
                // Offset of the value inside `line.raw`, so element spans are
                // absolute source positions.
                let value_offset = line.raw.len() - line.raw.trim_start().len() + key.len() + 1;
                expose = Some(parse_expose_list(line, value, value_offset)?);
            }
            other => {
                return Err(entry_error(
                    line,
                    &format!("unknown `mcp` setting `{other}` (the block takes only `expose`)"),
                ));
            }
        }
    }
    Err(span_error("unterminated `mcp` block", trimmed_span(header)))
}

pub(super) fn entry_error(line: &Line<'_>, message: &str) -> SchemaError {
    span_error(format!("mcp block: {message}"), trimmed_span(line))
}
