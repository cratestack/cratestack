//! Does the schema's `mcp { }` block agree with its attributes, and can the
//! declared scope exist at all?

use cratestack_core::Schema;

use super::{resources, tools};
use crate::diagnostics::{SchemaError, span_error};

/// ADR 0002: "An `@mcp`/`@@mcp` attribute in a schema with no `mcp { }`
/// block is an error." The block is the schema-level switch; an attribute
/// without it is a declaration nobody turned on.
pub(super) fn attributes_need_a_block(schema: &Schema, errors: &mut Vec<SchemaError>) {
    if schema.mcp.is_some() {
        return;
    }
    for (procedure, tool) in tools(schema) {
        errors.push(span_error(
            format!(
                "`@mcp(tool)` on procedure `{}` needs a top-level `mcp {{ expose tools }}` \
                 block: an MCP attribute in a schema with no `mcp {{ }}` block is an error \
                 (ADR 0002 § Validation)",
                procedure.name
            ),
            tool.span,
        ));
    }
    for (model, resource) in resources(schema) {
        errors.push(span_error(
            format!(
                "`@@mcp(resource: ...)` on model `{}` needs a top-level `mcp {{ expose \
                 resources }}` block: an MCP attribute in a schema with no `mcp {{ }}` block \
                 is an error (ADR 0002 § Validation)",
                model.name
            ),
            resource.span,
        ));
    }
}

/// The block exists but does not open the scope the attribute needs —
/// `@mcp(tool)` under a block with only `expose resources`, or the reverse.
/// Without this the attribute would be declared, validated and then served
/// by nothing.
pub(super) fn attributes_need_their_expose_line(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let Some(config) = &schema.mcp else {
        return;
    };
    if !config.exposes_tools() {
        for (procedure, tool) in tools(schema) {
            errors.push(span_error(
                format!(
                    "`@mcp(tool)` on procedure `{}` is not exposed: the `mcp {{ }}` block has \
                     no `expose tools` line (ADR 0002 § Schema surface)",
                    procedure.name
                ),
                tool.span,
            ));
        }
    }
    if !config.exposes_resources() {
        for (model, resource) in resources(schema) {
            errors.push(span_error(
                format!(
                    "`@@mcp(resource: ...)` on model `{}` is not exposed: the `mcp {{ }}` block \
                     has no `expose resources` line (ADR 0002 § Schema surface)",
                    model.name
                ),
                resource.span,
            ));
        }
    }
}

/// ADR 0002: "... and so is `mcp { expose tools }` with no `@mcp(tool)`
/// anywhere." Same for resources.
pub(super) fn expose_lines_must_be_used(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let Some(config) = &schema.mcp else {
        return;
    };
    if let Some(span) = config.expose_tools
        && tools(schema).next().is_none()
    {
        errors.push(span_error(
            "`expose tools` exposes nothing: no procedure carries `@mcp(tool)`. An `expose` \
             line that nothing uses is an error (ADR 0002 § Validation)",
            span,
        ));
    }
    if let Some(span) = config.expose_resources
        && resources(schema).next().is_none()
    {
        errors.push(span_error(
            "`expose resources` exposes nothing: no model carries `@@mcp(resource: ...)`. An \
             `expose` line that nothing uses is an error (ADR 0002 § Validation)",
            span,
        ));
    }
}

/// ADR 0002: "`@@mcp` is an error in a `db = None` schema, which has no
/// models. More generally, `mcp { expose resources }` is rejected wherever
/// resources cannot exist." The existing "no `model` under `provider =
/// \"none\"`" error already fires for such a model; this one names MCP so
/// the author learns that resources, not just models, are off the table.
pub(super) fn no_resources_without_a_database(schema: &Schema, errors: &mut Vec<SchemaError>) {
    if super::super::datasource_provider(schema) != Some("none") {
        return;
    }
    let reason = "a `datasource { provider = \"none\" }` (db = None) schema has no models, so \
                  it can expose no MCP resources — only tools (ADR 0002 § Validation, D3)";
    if let Some(span) = schema
        .mcp
        .as_ref()
        .and_then(|config| config.expose_resources)
    {
        errors.push(span_error(
            format!("`expose resources` is not allowed: {reason}"),
            span,
        ));
    }
    for (model, resource) in resources(schema) {
        errors.push(span_error(
            format!("`@@mcp` on model `{}` is not allowed: {reason}", model.name),
            resource.span,
        ));
    }
}
