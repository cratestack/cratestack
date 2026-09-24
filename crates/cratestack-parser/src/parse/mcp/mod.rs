//! Parsing of the MCP operator surface into typed IR (ADR 0002,
//! cratestack#1036): the `mcp { }` block, `@mcp(...)` on procedures and
//! `@@mcp(...)` on models. Cross-declaration rules are in `validate::mcp`.

mod args;
mod attribute;
mod block;
pub(crate) mod position;

pub(crate) use attribute::{extract_model_mcp, extract_procedure_mcp};
pub(crate) use block::parse_mcp_block;
