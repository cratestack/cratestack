//! The generated `cratestack_schema::mcp` module (ADR 0002 § Dispatch,
//! cratestack#1038): the static tool table and a `match` from tool name to
//! each procedure's generated `invoke_with_db`, implementing
//! `cratestack_mcp::McpTools`. The protocol side lives in `cratestack-mcp`.
//!
//! Laid out like `rpc_module`: this file assembles the module, and the two
//! halves are split by concern — [`table`] is the data an agent sees
//! (`tools/list`), [`dispatch`] is the code a call runs (`tools/call`).
//!
//! Emitted only when the schema exposes at least one tool, and reached only
//! when `cratestack-macros` has its `mcp` feature: without it,
//! `include::mcp_gate` stops a schema that declares any MCP before this
//! runs, so every `::cratestack::mcp::*` path below resolves through the
//! facade's `mcp` re-export.

mod dispatch;
mod table;

use std::collections::BTreeSet;

use quote::quote;

use super::super::mcp_gate::ToolPlan;

pub(super) fn build_mcp_module(
    tools: &[ToolPlan],
    auth_required: bool,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if tools.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let table = table::table_tokens(tools, auth_required);
    let dispatch = dispatch::dispatch_tokens(tools, bearing);

    quote! {
        pub mod mcp {
            //! This schema's MCP tools (ADR 0002): the `@mcp(tool)`
            //! procedures, in declaration order. Serve them with
            //! `::cratestack::mcp::StdioServer::new(tools(db, registry,
            //! resolvers), ctx)`, where `ctx` is the caller's identity —
            //! there is no default.
            //!
            //! Every call reaches its procedure through that procedure's
            //! generated `invoke_with_db`, the same function the REST and
            //! RPC handlers call, so `@allow`/`@deny` and any delegated
            //! `@authorize(...)` run before the implementation, and the
            //! `Authorized` witness the registry method requires makes a
            //! path that skipped them fail to compile (cratestack#512).

            #table
            #dispatch
        }
    }
}
