//! The generated `cratestack_schema::mcp` module (ADR 0002 § Dispatch,
//! cratestack#1038, cratestack#1040): the static tool and resource tables,
//! a `match` from tool name to each procedure's generated `invoke_with_db`,
//! and one from resource segment to the ORM calls REST's own handlers
//! make, implementing `cratestack_mcp::McpTools`. The protocol side lives
//! in `cratestack-mcp`.
//!
//! Laid out like `rpc_module`: this file assembles the module, and the
//! halves are split by concern — [`table`] and [`resources_table`] are the
//! data an agent sees (`tools/list`, `resources/list`), [`dispatch`] and
//! [`resources_dispatch`] the code a call or a read runs.
//!
//! Emitted only when the schema exposes at least one tool or resource, and
//! reached only when `cratestack-macros` has its `mcp` feature: without it,
//! `include::mcp_gate` stops a schema that declares any MCP before this
//! runs, so every `::cratestack::mcp::*` path below resolves through the
//! facade's `mcp` re-export.

mod dispatch;
mod resources_dispatch;
mod resources_table;
mod table;

use std::collections::BTreeSet;

use quote::quote;

use super::super::mcp_gate::McpPlan;

pub(super) fn build_mcp_module(
    plan: &McpPlan,
    auth_required: bool,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if plan.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let table = table::table_tokens(&plan.tools, auth_required);
    let resources_table = resources_table::resources_table_tokens(&plan.resources, auth_required);
    let resource_methods = resources_dispatch::resource_method_tokens(&plan.resources);
    let resource_support = resources_dispatch::resource_support_tokens(&plan.resources);
    let dispatch = dispatch::dispatch_tokens(&plan.tools, bearing, resource_methods);

    quote! {
        pub mod mcp {
            //! This schema's MCP surface (ADR 0002): the `@mcp(tool)`
            //! procedures and the `@@mcp(resource: ...)` models, in
            //! declaration order. Serve them with
            //! `::cratestack::mcp::StdioServer::new(tools(db, registry,
            //! resolvers), ctx)`, where `ctx` is the caller's identity —
            //! there is no default, and an anonymous one is refused.
            //!
            //! Every call reaches its procedure through that procedure's
            //! generated `invoke_with_db`, the same function the REST and
            //! RPC handlers call, so `@allow`/`@deny` and any delegated
            //! `@authorize(...)` run before the implementation, and the
            //! `Authorized` witness the registry method requires makes a
            //! path that skipped them fail to compile (cratestack#512).
            //! Every resource read is the ORM call REST's `GET` handlers
            //! make, under `ctx`, so `@@allow("read", ...)` is in its SQL.

            #table
            #resources_table
            #dispatch
            #resource_support
        }
    }
}
