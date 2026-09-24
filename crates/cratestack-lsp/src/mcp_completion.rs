//! Completion entries for the MCP surface (ADR 0002, cratestack#1036).
//!
//! Kept out of `completion.rs`, which is already past the 200-line ceiling.
//! Each entry carries a detail string because the attributes' argument shapes
//! are the part authors get wrong: the bare `@mcp(tool)` form, and that a
//! model takes `resource:` while a procedure takes `tool` — the parser
//! rejects the swapped spellings, so the popup says which is which up front.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

const ENTRIES: [(&str, &str); 3] = [
    (
        "@mcp",
        "procedure attribute: expose it as an MCP tool — `@mcp(tool)` uses the procedure name, \
         `@mcp(tool: \"name\", description: \"...\")` names it; needs an `@allow(...)` and \
         `tools` in the block's `expose` list (ADR 0002)",
    ),
    (
        "@@mcp",
        "model attribute: expose it as a read-only MCP resource — `@@mcp(resource: \"segment\")`, \
         optional `max_page_size:` 1-200; needs a read `@@allow` and \
         `resources` in the block's `expose` list (ADR 0002)",
    ),
    (
        "expose = [tools, resources]",
        "inside `mcp { }`: which MCP kinds to serve — `tools` (procedures marked \
         `@mcp(tool)`), `resources` (models marked `@@mcp(resource: ...)`, not in a \
         `provider = \"none\"` schema), or both",
    ),
];

pub(crate) fn completion_items() -> impl Iterator<Item = CompletionItem> {
    ENTRIES.into_iter().map(|(label, detail)| CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail.to_owned()),
        ..CompletionItem::default()
    })
}
