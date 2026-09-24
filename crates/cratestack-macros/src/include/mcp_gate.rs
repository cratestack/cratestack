//! The MCP release gate (ADR 0002 Q4, D3; cratestack#1036).
//!
//! Phase 1 of cratestack#1033 parses and validates `mcp { }`, `@mcp(tool)`
//! and `@@mcp(resource: ...)`, but nothing serves them yet. Letting such a
//! schema compile would ship the exact failure Q4 exists to prevent: an
//! attribute that parses and does nothing, which is how `@no_idempotency`
//! sat inert for two release cycles. So each role decides here, once:
//!
//! - **server** (`include_server_schema!`): `compile_error!` naming
//!   cratestack#1033, until the phase-3 runtime lands and removes this arm;
//! - **embedded** (`include_embedded_schema!`): `compile_error!` for good —
//!   the embedded role enforces no policy, so an MCP surface there could not
//!   keep ADR 0002's central promise (D3), and no later phase changes that;
//! - **client** (`include_client_schema!`): no gate at all. A client treats
//!   another service's schema as a contract; that service's MCP exposure is
//!   not the client's concern, so the declarations are accepted and ignored.
//!   The client composer never reads `Schema.mcp`/`Model.mcp`/`Procedure.mcp`;
//!   `cratestack-client`'s `mcp_declarations_are_ignored` test pins that.
//!
//! Runs first in both gated composers, straight after the schema parses, so
//! an MCP schema fails with this message rather than whichever unrelated
//! guard happens to run earlier.

use proc_macro::TokenStream;
use syn::LitStr;

use cratestack_core::Schema;

pub(super) fn guard_server_mcp(schema_path: &LitStr, schema: &Schema) -> Result<(), TokenStream> {
    let Some(declared) = mcp_declarations(schema) else {
        return Ok(());
    };
    Err(error(
        schema_path,
        format!(
            "schema declares an MCP surface ({declared}), but the MCP runtime has not shipped: \
             include_server_schema! rejects MCP declarations until it does, so none can parse \
             and then serve nothing (ADR 0002 Q4). Tracking: cratestack#1033 \
             (https://github.com/cratestack/cratestack/issues/1033) — remove the MCP \
             declarations to build this schema today."
        ),
    ))
}

pub(super) fn guard_embedded_mcp(schema_path: &LitStr, schema: &Schema) -> Result<(), TokenStream> {
    let Some(declared) = mcp_declarations(schema) else {
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

/// A short, human list of what the schema declares, or `None` when it
/// declares no MCP at all. Checks all three IR slots rather than only the
/// block: validation already guarantees an attribute implies a block, but
/// this gate is what keeps MCP from being inert, so it does not lean on that.
fn mcp_declarations(schema: &Schema) -> Option<String> {
    let mut declared = Vec::new();
    if schema.mcp.is_some() {
        declared.push("an `mcp { }` block".to_owned());
    }
    for procedure in &schema.procedures {
        if procedure.mcp.is_some() {
            declared.push(format!("`@mcp(tool)` on procedure `{}`", procedure.name));
        }
    }
    for model in &schema.models {
        if model.mcp.is_some() {
            declared.push(format!("`@@mcp(resource: ...)` on model `{}`", model.name));
        }
    }
    (!declared.is_empty()).then(|| declared.join(", "))
}

fn error(schema_path: &LitStr, message: String) -> TokenStream {
    TokenStream::from(syn::Error::new(schema_path.span(), message).to_compile_error())
}

#[cfg(test)]
mod tests {
    // The guards return a `proc_macro::TokenStream`, which panics outside a
    // proc-macro invocation (see `datasource_guard`'s tests), so the
    // predicate is what is unit-tested here; `tests/ui_mcp.rs` pins the real
    // compile errors.
    use super::mcp_declarations;

    const MCP_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
}

mcp {
  expose = [tools, resources]
}

type Args {
  n Int
}

model Post {
  id Int @id

  @@allow("read", true)
  @@mcp(resource: "posts")
}

procedure getFeed(args: Args): Post[]
  @allow(true)
  @mcp(tool)
"#;

    #[test]
    fn names_every_mcp_declaration() {
        let schema = cratestack_parser::parse_schema(MCP_SCHEMA).expect("valid MCP schema");
        assert_eq!(
            mcp_declarations(&schema).as_deref(),
            Some(
                "an `mcp { }` block, `@mcp(tool)` on procedure `getFeed`, \
                 `@@mcp(resource: ...)` on model `Post`"
            )
        );
    }

    #[test]
    fn a_schema_without_mcp_passes() {
        let schema = cratestack_parser::parse_schema("model Post {\n  id Int @id\n}\n")
            .expect("valid schema");
        assert_eq!(mcp_declarations(&schema), None);
    }
}
