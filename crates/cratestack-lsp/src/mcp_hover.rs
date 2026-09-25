//! Hover for the MCP surface (ADR 0002, cratestack#1036): the `mcp { }`
//! block, each element of its `expose` list, its `name` (cratestack#1040),
//! `@mcp(...)` on a procedure and `@@mcp(...)` on a model.
//!
//! Every target is a span the parser recorded in the typed IR, so hover shows
//! what the parser *resolved* — most usefully the tool name a bare
//! `@mcp(tool)` defaulted to, which appears nowhere in the source.

use cratestack_core::{McpConfig, Schema};

use crate::state::SymbolInfo;
use crate::text::span_contains;

pub(crate) fn hover_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    if let Some(config) = &schema.mcp
        && span_contains(config.span, offset)
    {
        return Some(block_symbol(schema, config, offset));
    }
    for procedure in &schema.procedures {
        let Some(tool) = procedure.mcp.as_ref() else {
            continue;
        };
        if span_contains(tool.span, offset) {
            let origin = if tool.tool_name_defaulted {
                ", named after the procedure"
            } else {
                ""
            };
            return Some(SymbolInfo {
                kind: "mcp tool",
                name: tool.tool_name.clone(),
                detail: format!("procedure `{}` as MCP tool{origin}", procedure.name),
                docs: tool.description.iter().cloned().collect(),
                selection_span: tool.span,
            });
        }
    }
    for model in &schema.models {
        let Some(resource) = model.mcp.as_ref() else {
            continue;
        };
        if span_contains(resource.span, offset) {
            let page = resource
                .max_page_size
                .unwrap_or(cratestack_core::MCP_MAX_PAGE_SIZE);
            return Some(SymbolInfo {
                kind: "mcp resource",
                name: resource.resource.clone(),
                detail: format!(
                    "model `{}` as a read-only MCP resource, at most {page} records per page",
                    model.name
                ),
                docs: Vec::new(),
                selection_span: resource.span,
            });
        }
    }
    None
}

fn block_symbol(schema: &Schema, config: &McpConfig, offset: usize) -> SymbolInfo {
    let tools = schema
        .procedures
        .iter()
        .filter_map(|procedure| procedure.mcp.as_ref())
        .map(|tool| tool.tool_name.as_str())
        .collect::<Vec<_>>();
    let resources = schema
        .models
        .iter()
        .filter_map(|model| model.mcp.as_ref())
        .map(|resource| resource.resource.as_str())
        .collect::<Vec<_>>();
    let listed = |names: &[&str]| {
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    };
    if let Some(span) = config
        .expose_tools
        .filter(|span| span_contains(*span, offset))
    {
        return expose_symbol("tools", format!("tools: {}", listed(&tools)), span);
    }
    if let Some(span) = config
        .expose_resources
        .filter(|span| span_contains(*span, offset))
    {
        let detail = format!("resources: {}", listed(&resources));
        return expose_symbol("resources", detail, span);
    }
    if let Some(name) = config
        .name
        .as_ref()
        .filter(|name| span_contains(name.span, offset))
    {
        // The URIs the name produces, which is the one thing it is for.
        let uris = resources
            .iter()
            .map(|segment| format!("cratestack://{}/{segment}", name.value))
            .collect::<Vec<_>>();
        let uris = uris.iter().map(String::as_str).collect::<Vec<_>>();
        return SymbolInfo {
            kind: "mcp name",
            name: name.value.clone(),
            detail: format!("MCP resource URIs: {}", listed(&uris)),
            docs: Vec::new(),
            selection_span: name.span,
        };
    }
    SymbolInfo {
        kind: "mcp block",
        name: "mcp".to_owned(),
        detail: format!(
            "MCP operator surface — tools: {}; resources: {}",
            listed(&tools),
            listed(&resources)
        ),
        docs: config.docs.clone(),
        selection_span: config.span,
    }
}

fn expose_symbol(name: &str, detail: String, span: cratestack_core::SourceSpan) -> SymbolInfo {
    SymbolInfo {
        kind: "mcp exposed kind",
        name: name.to_owned(),
        detail,
        docs: Vec::new(),
        selection_span: span,
    }
}
