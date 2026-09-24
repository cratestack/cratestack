//! What each role does with a schema's MCP declarations (ADR 0002 Q4, D3;
//! cratestack#1036, cratestack#1038).
//!
//! - **server** (`include_server_schema!`): the tools are served by the
//!   generated `mcp` module (`include/server/mcp_module/`) when
//!   `cratestack-macros` has its `mcp` feature, forwarded from
//!   `cratestack-pg`/`cratestack-api`'s own. Without the feature, and for
//!   resources in any case, this is still the Q4 `compile_error!`: an
//!   attribute that parsed and then served nothing is what Q4 exists to
//!   prevent (`@no_idempotency` sat inert for two release cycles that way).
//!   Resources are phase 5 of cratestack#1033, so they stay gated until it
//!   lands. See [`plan::server_plan`] for the order of the checks.
//! - **embedded** (`include_embedded_schema!`): `compile_error!` for good —
//!   the embedded role enforces no policy, so an MCP surface there could not
//!   keep ADR 0002's central promise (D3), and no later phase changes that.
//! - **client** (`include_client_schema!`): no gate at all. A client treats
//!   another service's schema as a contract; that service's MCP exposure is
//!   not the client's concern, so the declarations are accepted and ignored.
//!   `cratestack-client`'s `mcp_declarations_are_ignored` test pins that.
//!
//! **Why a proc-macro feature, not the consumer's.** A proc-macro cannot see
//! the invoking crate's Cargo features, only its own (`extension_gate.rs`'s
//! module doc has the measurement). The facades forward `mcp` down to
//! `cratestack-macros/mcp`, the mechanism `rate_limit`/`pgvector` use.

mod plan;
#[cfg(test)]
mod tests;

use proc_macro::TokenStream;
use syn::LitStr;

use cratestack_core::Schema;

use super::decimal_arg::resolve_decimal_backend;
use crate::shared::decimal_backend::DecimalBackend;

pub(super) use plan::ToolPlan;

/// The tools the server macro must generate, in declaration order — empty
/// when the schema declares no MCP at all — or the compile error that stops
/// it. Runs first in the server composer, straight after the schema
/// parses, so an MCP schema fails with this message rather than whichever
/// unrelated guard happens to run earlier. The decimal backend is resolved
/// here only when there are tools, so a schema without MCP sees the same
/// guard order as before.
pub(super) fn guard_server_mcp(
    schema_path: &LitStr,
    schema: &Schema,
    decimal: Option<DecimalBackend>,
) -> Result<Vec<ToolPlan>, TokenStream> {
    if plan::mcp_declarations(schema).is_none() {
        return Ok(Vec::new());
    }
    let decimal = resolve_decimal_backend(schema_path, schema, decimal)?;
    plan::server_plan(schema, decimal, cfg!(feature = "mcp"))
        .map_err(|message| error(schema_path, message))
}

pub(super) fn guard_embedded_mcp(schema_path: &LitStr, schema: &Schema) -> Result<(), TokenStream> {
    let Some(declared) = plan::mcp_declarations(schema) else {
        return Ok(());
    };
    Err(error(
        schema_path,
        format!(
            "schema declares an MCP surface ({declared}), but include_embedded_schema! never \
             serves MCP: the embedded role enforces no `@allow`/`@@allow` policy, so an MCP \
             surface here could not keep MCP's policy guarantee (ADR 0002 D3). Remove the MCP \
             declarations, or consume this schema through include_server_schema!."
        ),
    ))
}

fn error(schema_path: &LitStr, message: String) -> TokenStream {
    TokenStream::from(syn::Error::new(schema_path.span(), message).to_compile_error())
}
