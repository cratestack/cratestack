//! `expose = [tools, resources]` inside `mcp { }`.
//!
//! Each element keeps its own span, so a rule about one element — "`tools`
//! exposes nothing", "`resources` needs a database" — points at that word,
//! not at the whole line.

use cratestack_core::SourceSpan;

use super::block::entry_error;
use crate::diagnostics::{SchemaError, span_error};
use crate::line_helpers::Line;

/// The parsed list: the span of each element that was present.
pub(super) struct ExposeList {
    pub(super) tools: Option<SourceSpan>,
    pub(super) resources: Option<SourceSpan>,
}

/// `value` is the text after `=`, and `value_offset` its byte offset in
/// `line.raw`. The list must sit on one line: every other config value does,
/// and a two-element list has no reason to wrap.
pub(super) fn parse_expose_list(
    line: &Line<'_>,
    value: &str,
    value_offset: usize,
) -> Result<ExposeList, SchemaError> {
    let lead = value.len() - value.trim_start().len();
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Err(entry_error(
            line,
            &format!(
                "`expose = {trimmed}` must be a one-line list: `expose = [tools]`, \
                 `expose = [resources]` or `expose = [tools, resources]`"
            ),
        ));
    };
    if inner.trim().is_empty() {
        return Err(entry_error(
            line,
            "`expose = []` is empty, so it exposes nothing: list `tools`, `resources` or both",
        ));
    }
    // Absolute offset of `inner`'s first byte (just past the `[`).
    let inner_start = line.start + value_offset + lead + 1;
    let mut list = ExposeList {
        tools: None,
        resources: None,
    };
    let mut at = 0usize;
    for part in inner.split(',') {
        let element = part.trim();
        let start = inner_start + at + (part.len() - part.trim_start().len());
        at += part.len() + 1;
        let span = SourceSpan {
            start,
            end: start + element.len(),
            line: line.number,
        };
        let fail = |message: String| span_error(format!("mcp block: {message}"), span);
        let slot = match element {
            "tools" => &mut list.tools,
            "resources" => &mut list.resources,
            "" => {
                return Err(fail(
                    "`expose` has an empty element (a stray `,`)".to_owned(),
                ));
            }
            "procedures" => {
                return Err(fail(
                    "`procedures` was renamed to `tools`, MCP's own word for an invocable \
                     procedure: write `expose = [tools]` (ADR 0002 § Schema surface)"
                        .to_owned(),
                ));
            }
            other => {
                return Err(fail(format!(
                    "unknown `expose` element `{other}` (expected `tools` or `resources`)"
                )));
            }
        };
        if slot.is_some() {
            return Err(fail(format!(
                "`{element}` is listed more than once in `expose`"
            )));
        }
        *slot = Some(span);
    }
    Ok(list)
}

/// Migration hint for the pre-2026-09-24 line form (`expose` followed by
/// bare words, one per line), which ADR 0002's first revision used and the
/// maintainer replaced with `expose = [...]` so the block is `key = value`
/// like every other config block. Returns the message showing the new
/// spelling for this line, or `None` if `entry` is not that form.
pub(super) fn old_line_form_hint(entry: &str) -> Option<String> {
    let words = entry.strip_prefix("expose ")?.split_whitespace();
    let elements = words
        .map(|word| if word == "procedures" { "tools" } else { word })
        .collect::<Vec<_>>();
    Some(format!(
        "`{entry}` is the old line form; the block is `key = value` now: write \
         `expose = [{}]`, with every exposed kind in the one list (for example \
         `expose = [tools, resources]`)",
        elements.join(", ")
    ))
}
