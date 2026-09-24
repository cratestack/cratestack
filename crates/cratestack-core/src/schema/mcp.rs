//! Typed IR for a schema's MCP operator surface (ADR 0002, cratestack#1033
//! phase 1, cratestack#1036).
//!
//! Three declarations feed it: the top-level `mcp { name = "..."  expose =
//! [tools, resources] }` block ([`McpConfig`], [`McpName`]), `@mcp(tool[: "name"][, description:
//! "..."])` on a procedure ([`ProcedureMcpExposure`]), and `@@mcp(resource:
//! "segment"[, max_page_size: N])` on a model ([`ModelMcpExposure`]).
//!
//! These are typed rather than left as `Attribute { raw }` / a raw-text
//! `ConfigBlock` because MCP exposure decides what an agent can reach: a
//! declaration that parses and is then read by nothing is the failure mode
//! this IR exists to rule out (ADR 0002 Q4 — `@no_idempotency` sat inert for
//! two release cycles exactly that way). `cratestack-parser` extracts the
//! `@mcp`/`@@mcp` attributes *out of* the raw attribute lists when it builds
//! these, so there is one representation to read, never a raw copy that a
//! consumer could re-parse and disagree with.
//!
//! Every field is `Option` with `#[serde(default, skip_serializing_if)]` on
//! its owner, so an IR snapshot written before MCP existed (committed
//! `migrations/*/schema.snapshot.json`) deserializes unchanged and a schema
//! that declares no MCP serializes byte-identically to before.

use serde::{Deserialize, Serialize};

use super::SourceSpan;

/// Hard ceiling on a collection resource's page size (ADR 0002 Q3). A model
/// may lower its own maximum with `max_page_size:`, never raise it past this;
/// a larger value is a validation error, not a clamp.
pub const MCP_MAX_PAGE_SIZE: u32 = 200;

/// The top-level `mcp { }` block.
///
/// Each `expose` flag is the span of its element in `expose = [...]` rather
/// than a `bool`, so that "an exposed kind nothing uses" can point at that
/// element — and so presence and position can never disagree the way a
/// `bool` plus a separate span could.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConfig {
    pub docs: Vec<String>,
    /// `Some(span of the `tools` element)` when `expose` lists it.
    pub expose_tools: Option<SourceSpan>,
    /// `Some(span of the `resources` element)` when `expose` lists it.
    pub expose_resources: Option<SourceSpan>,
    /// `name = "..."`: the `<name>` in every resource URI,
    /// `cratestack://<name>/<segment>/{id}` (maintainer decision on
    /// cratestack#1040). Present exactly when `expose` lists `resources`;
    /// the parser refuses it missing there and present anywhere else.
    ///
    /// Stated in the schema rather than derived from the `.cstack` file's
    /// name, as phase 5 first did, so renaming or moving the file does not
    /// change a URI an agent already holds, and two servers whose files
    /// share a name can still be told apart. `serde(default)` so a snapshot
    /// from before the key existed still deserializes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<McpName>,
    /// The whole block, header to closing brace. This is the span a `part of`
    /// file's rejection of `mcp { }` reports (cratestack#993).
    pub span: SourceSpan,
}

/// The `name = "..."` entry of the `mcp { }` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpName {
    /// The string's contents, a lowercase DNS label once parsed (`[a-z0-9-]`,
    /// 1 to 63 characters, no `-` at either end; cratestack#1040), because
    /// it is a URI's host: lowercase so the one spelling is the only
    /// spelling (the host is compared exactly), and no `.`, `:`, `@` or `%`,
    /// so it never needs percent-encoding and never reads as a port,
    /// userinfo or a dotted host name.
    pub value: String,
    /// The whole `name = "..."` entry.
    pub span: SourceSpan,
}

impl McpConfig {
    pub fn exposes_tools(&self) -> bool {
        self.expose_tools.is_some()
    }

    pub fn exposes_resources(&self) -> bool {
        self.expose_resources.is_some()
    }
}

/// `@mcp(tool ...)` on a procedure: expose it as an MCP tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureMcpExposure {
    /// The MCP tool name — the `tool: "..."` value, or the procedure's own
    /// name as written for a bare `@mcp(tool)` (ADR 0002 Q2).
    pub tool_name: String,
    /// `true` when `tool_name` was defaulted from the procedure name. Kept so
    /// a malformed defaulted name can be reported as such: the author never
    /// typed that string, and should be told where it came from.
    pub tool_name_defaulted: bool,
    pub description: Option<String>,
    /// The `@mcp(...)` attribute itself.
    pub span: SourceSpan,
}

/// `@@mcp(resource: "...")` on a model: expose it as a read-only MCP
/// resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelMcpExposure {
    /// The URI segment in `cratestack://<name>/<segment>`. Author-chosen,
    /// never derived from the table or model name, so the database layout is
    /// not exposed (ADR 0002 § Resources).
    pub resource: String,
    /// A per-resource page-size ceiling, `1..=`[`MCP_MAX_PAGE_SIZE`] once
    /// validated. `None` means the framework maximum applies.
    pub max_page_size: Option<u32>,
    /// The `@@mcp(...)` attribute itself.
    pub span: SourceSpan,
}
