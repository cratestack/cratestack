//! Cross-declaration MCP rules (ADR 0002 § Validation, cratestack#1036).
//!
//! MCP exposure changes what an agent can reach, so every rule here is a
//! hard error — stricter than the general "unknown attributes are inert"
//! posture (#679), on purpose. The attribute *shapes* were already checked
//! when `parse::mcp` built the typed IR; what is left needs more than one
//! declaration to decide.
//!
//! Each rule is its own function, pushed independently into the collecting
//! stage, so an editor sees every MCP mistake at once and — the property the
//! story's mutation evidence depends on — deleting one rule's check makes
//! exactly that rule's tests fail.
//!
//! Runs in stage 2 of `validate_schema_collecting`: none of it depends on
//! type resolution, and the `db = None` rule has to sit next to the existing
//! "no model under `provider = \"none\"`" check so both are reported.

mod names;
mod placement;
mod policy;
mod scope;

use cratestack_core::Schema;

use crate::diagnostics::SchemaError;

pub(super) fn validate_mcp_collecting(schema: &Schema, errors: &mut Vec<SchemaError>) {
    scope::attributes_need_a_block(schema, errors);
    scope::attributes_need_their_expose_line(schema, errors);
    scope::expose_lines_must_be_used(schema, errors);
    scope::no_resources_without_a_database(schema, errors);
    names::tool_names_are_well_formed(schema, errors);
    names::resource_segments_are_well_formed(schema, errors);
    names::max_page_size_is_in_range(schema, errors);
    names::tool_names_are_unique(schema, errors);
    names::resource_segments_are_unique(schema, errors);
    policy::resources_need_a_read_allow(schema, errors);
    policy::tools_need_an_allow(schema, errors);
    placement::no_mcp_on_fields(schema, errors);
    placement::no_mcp_on_views(schema, errors);
}

/// Every procedure that carries `@mcp(tool ...)`, with its exposure.
fn tools(
    schema: &Schema,
) -> impl Iterator<
    Item = (
        &cratestack_core::Procedure,
        &cratestack_core::ProcedureMcpExposure,
    ),
> {
    schema
        .procedures
        .iter()
        .filter_map(|procedure| procedure.mcp.as_ref().map(|mcp| (procedure, mcp)))
}

/// Every model that carries `@@mcp(resource: ...)`, with its exposure.
fn resources(
    schema: &Schema,
) -> impl Iterator<Item = (&cratestack_core::Model, &cratestack_core::ModelMcpExposure)> {
    schema
        .models
        .iter()
        .filter_map(|model| model.mcp.as_ref().map(|mcp| (model, mcp)))
}
