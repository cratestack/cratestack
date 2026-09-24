//! Tool names, resource segments and page sizes: well-formed and unique.

use std::collections::BTreeMap;

use cratestack_core::{MCP_MAX_PAGE_SIZE, Schema, SourceSpan};

use super::{resources, tools};
use crate::diagnostics::{SchemaError, span_error};

/// The MCP specification's tool-name grammar, `[A-Za-z0-9_.-]{1,128}`.
fn is_valid_tool_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// ADR 0002's resource-segment grammar, `[a-z0-9-]+`.
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Checked on the *resulting* name, given or defaulted: procedure names are
/// not length-bounded, so a bare `@mcp(tool)` can produce a name the spec
/// rejects even though the author never typed one.
pub(super) fn tool_names_are_well_formed(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (procedure, tool) in tools(schema) {
        if is_valid_tool_name(&tool.tool_name) {
            continue;
        }
        let origin = if tool.tool_name_defaulted {
            " (defaulted from the procedure name by the bare `@mcp(tool)`; name the tool \
             explicitly with `@mcp(tool: \"...\")`)"
        } else {
            ""
        };
        errors.push(span_error(
            format!(
                "MCP tool name `{}` on procedure `{}` is malformed: a tool name must match \
                 `[A-Za-z0-9_.-]{{1,128}}` (ADR 0002 § Validation){origin}",
                tool.tool_name, procedure.name
            ),
            tool.span,
        ));
    }
}

pub(super) fn resource_segments_are_well_formed(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (model, resource) in resources(schema) {
        if !is_valid_segment(&resource.resource) {
            errors.push(span_error(
                format!(
                    "MCP resource segment `{}` on model `{}` is malformed: a segment must \
                     match `[a-z0-9-]+` (ADR 0002 § Validation)",
                    resource.resource, model.name
                ),
                resource.span,
            ));
        }
    }
}

/// Q3: a resource may lower the page-size maximum, never raise it — so a
/// value above the ceiling is an error, not a clamp.
pub(super) fn max_page_size_is_in_range(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (model, resource) in resources(schema) {
        let Some(size) = resource.max_page_size else {
            continue;
        };
        if !(1..=MCP_MAX_PAGE_SIZE).contains(&size) {
            errors.push(span_error(
                format!(
                    "`max_page_size: {size}` on model `{}` is out of range: it must be an \
                     integer from 1 to {MCP_MAX_PAGE_SIZE}, and a value above \
                     {MCP_MAX_PAGE_SIZE} is an error, not a clamp (ADR 0002 § Validation, Q3)",
                    model.name
                ),
                resource.span,
            ));
        }
    }
}

pub(super) fn tool_names_are_unique(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let entries = tools(schema).map(|(procedure, tool)| {
        (
            tool.tool_name.as_str(),
            format!("procedure `{}`", procedure.name),
            tool.span,
        )
    });
    report_duplicates(entries, "MCP tool name", errors);
}

pub(super) fn resource_segments_are_unique(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let entries = resources(schema).map(|(model, resource)| {
        (
            resource.resource.as_str(),
            format!("model `{}`", model.name),
            resource.span,
        )
    });
    report_duplicates(entries, "MCP resource segment", errors);
}

fn report_duplicates<'a>(
    entries: impl Iterator<Item = (&'a str, String, SourceSpan)>,
    what: &str,
    errors: &mut Vec<SchemaError>,
) {
    let mut first_owner: BTreeMap<&str, String> = BTreeMap::new();
    for (key, owner, span) in entries {
        if let Some(first) = first_owner.get(key) {
            errors.push(span_error(
                format!(
                    "duplicate {what} `{key}` on {owner}: {first} already uses it. Two tools \
                     with the same name, or two resources with the same segment, are an error \
                     (ADR 0002 § Validation)"
                ),
                span,
            ));
        } else {
            first_owner.insert(key, owner);
        }
    }
}
