//! The top-level `mcp { expose tools / expose resources }` block (ADR 0002
//! § Schema surface).
//!
//! Before cratestack#1036 this went through `parse_simple_config_block` and
//! became opaque text lines in `Schema.config_blocks` that nothing read, so
//! `mcp { anything at all }` parsed. Every line is now one of the two
//! `expose` settings or an error.

use cratestack_core::{McpConfig, SourceSpan};

use crate::diagnostics::{SchemaError, span_error};
use crate::line_helpers::{Line, span_from_lines, trimmed_span};

pub(crate) fn parse_mcp_block(
    lines: &[Line<'_>],
    start: usize,
    docs: Vec<String>,
    previous: Option<&McpConfig>,
) -> Result<(McpConfig, usize), SchemaError> {
    let header = &lines[start];
    let mut expose_tools: Option<SourceSpan> = None;
    let mut expose_resources: Option<SourceSpan> = None;
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
            if expose_tools.is_none() && expose_resources.is_none() {
                return Err(span_error(
                    "empty `mcp { }` block: it exposes nothing, so it would turn MCP on for \
                     no declaration; add `expose tools` and/or `expose resources`",
                    span,
                ));
            }
            let config = McpConfig {
                docs,
                expose_tools,
                expose_resources,
                span,
            };
            return Ok((config, cursor));
        }
        if line.trimmed.is_empty() || line.trimmed.starts_with("//") {
            continue;
        }
        let slot = match line
            .trimmed
            .split_whitespace()
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["expose", "tools"] => &mut expose_tools,
            ["expose", "resources"] => &mut expose_resources,
            ["expose", "procedures"] => {
                return Err(entry_error(
                    line,
                    "`expose procedures` was renamed to `expose tools`, MCP's own word for an \
                     invocable procedure (ADR 0002 § Schema surface)",
                ));
            }
            ["expose", other] => {
                return Err(entry_error(
                    line,
                    &format!("unknown `expose` target `{other}` (expected `tools` or `resources`)"),
                ));
            }
            _ => {
                return Err(entry_error(
                    line,
                    &format!(
                        "unsupported `mcp` block entry `{}` (expected `expose tools` or \
                         `expose resources`, one per line)",
                        line.trimmed
                    ),
                ));
            }
        };
        if slot.is_some() {
            return Err(entry_error(
                line,
                &format!("`{}` is declared more than once", line.trimmed),
            ));
        }
        *slot = Some(trimmed_span(line));
    }
    Err(span_error("unterminated `mcp` block", trimmed_span(header)))
}

fn entry_error(line: &Line<'_>, message: &str) -> SchemaError {
    span_error(format!("mcp block: {message}"), trimmed_span(line))
}
