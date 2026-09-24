//! L4 — the MCP binding. Serves a schema's `@mcp(tool)` procedures to agents
//! over the Model Context Protocol (ADR 0002, epic cratestack#1033, phase 3:
//! cratestack#1038).
//!
//! You do not depend on this crate directly. Turn on the `mcp` feature of
//! `cratestack-pg` or `cratestack-api` (ADR 0002 D3), and the schema macro
//! generates a `cratestack_schema::mcp` module whose `tools(...)` value
//! implements [`McpTools`]. Hand that to [`StdioServer`].
//!
//! # Where the policy check happens, and why it cannot be skipped
//!
//! This crate owns the protocol: `rmcp`'s [`rmcp::ServerHandler`], the
//! pinned protocol version, error mapping and the stdio transport. It never
//! runs a procedure itself. The generated [`McpTools::execute`] does, and it
//! can only reach an implementation through that procedure's generated
//! `invoke_with_db`, because the `ProcedureRegistry` method requires the
//! `Authorized` witness only `invoke_with_db` can build (cratestack#512). A
//! tool table that tried to skip `@allow` would not compile. Row policy is
//! compiled into the ORM's SQL, so it applies to whatever the implementation
//! reads. MCP adds no bypass and no MCP-specific identity (ADR 0002 § Decision).
//!
//! # What this crate adds on top: L3 admission
//!
//! Between decoding the arguments and running the tool, a call passes the
//! same `cratestack_exec::OpExecutor` admission REST and RPC use (ADR 0002 D2,
//! ADR 0015 amendment 2026-09-24): rate limiting, then idempotency. Both are
//! opt-in and application-built, exactly as `cratestack-axum` installs
//! `RateLimitLayer`/`IdempotencyLayer`: without
//! [`StdioServer::with_executor`], nothing is limited and nothing is
//! reserved. A failing rate-limit store follows
//! [`StoreErrorPolicy`], set with [`StdioServer::with_store_error_policy`]
//! and defaulting to HTTP's. An idempotency key arrives in
//! `_meta["dev.cratestack/idempotencyKey"]` (ADR 0002 Q6); see
//! `src/idempotency.rs`.
//!
//! # Protocol version
//!
//! Pinned to [`PROTOCOL_VERSION`] (`2026-07-28`): no `initialize`, no
//! session, `server/discover` required. `rmcp` 3.4's `ProtocolVersion::LATEST`
//! still names `2025-11-25`, so relying on its default would silently
//! advertise the older revision. The server advertises exactly one version,
//! so a legacy `initialize` is refused rather than negotiated down.

mod admission;
mod call;
mod fingerprint;
mod idempotency;
mod listing;
mod result;
mod server;
mod stdio;
mod table;
#[cfg(test)]
mod tests_idempotency;
#[cfg(test)]
mod tests_listing;

/// Re-exported so an application configuring MCP admission names both
/// without a direct `cratestack-exec` dependency. `StoreErrorPolicy` is the
/// same type `cratestack_axum::ratelimit` re-exports (cratestack#1038).
pub use cratestack_exec::{DEFAULT_STORE_TIMEOUT, OpExecutor, StoreErrorPolicy};
pub use listing::ToolTableError;
pub use server::McpServer;
pub use stdio::{ServeError, StdioServer};
pub use table::{ArgumentsError, McpTools, ToolDescriptor, decode_arguments, encode_output};

/// The one MCP revision this server speaks (ADR 0002 § Transports).
pub const PROTOCOL_VERSION: &str = "2026-07-28";

/// The vendor `_meta` key a `tools/call` may carry an idempotency key under
/// (ADR 0002 Q6). Reverse-DNS under `cratestack.dev`, as the spec asks of
/// vendor keys.
pub const IDEMPOTENCY_KEY_META: &str = "dev.cratestack/idempotencyKey";

/// Set to `true` in a replayed result's `_meta`, the MCP counterpart of the
/// `idempotency-replayed: true` header `cratestack-axum` adds on HTTP, so a
/// caller can tell a replay from a live run.
pub const IDEMPOTENCY_REPLAYED_META: &str = "dev.cratestack/idempotencyReplayed";
