//! The pure half of the server gate: what is declared, whether it can be
//! served, and each tool's JSON Schemas. Returns `String` errors rather
//! than a `proc_macro::TokenStream` (which panics outside a real macro
//! invocation), so `tests.rs` can drive both feature states directly.

use cratestack_core::{Procedure, Schema};

use crate::json_schema::{JsonSchemaError, procedure_input_schema, procedure_output_schema};
use crate::shared::decimal_backend::DecimalBackend;

/// One `@mcp(tool)` procedure, with the schemas the generated table embeds.
pub(in crate::include) struct ToolPlan {
    pub(in crate::include) procedure: Procedure,
    pub(in crate::include) name: String,
    pub(in crate::include) description: Option<String>,
    /// JSON Schema 2020-12, serialized.
    pub(in crate::include) input: String,
    /// `None` when the output is not an object (MCP's `outputSchema` must be).
    pub(in crate::include) output: Option<String>,
}

/// In this order, each a hard error:
///
/// 1. every tool's input and output schema must generate. A type with no
///    faithful mapping (`Json`, `FindMany`, `Vector`, `Geography`,
///    `Geometry`) is refused, never advertised as `{}` (ADR 0002 § Tools).
///    First, so the author hears about a tool that can never be served
///    before being told to turn a feature on for it;
/// 2. the `mcp` feature must be on — without it nothing would serve the
///    declarations (Q4);
/// 3. no resources: they are phase 5, and until it ships they would parse
///    and serve nothing (Q4 again).
pub(super) fn server_plan(
    schema: &Schema,
    decimal: Option<DecimalBackend>,
    feature_enabled: bool,
) -> Result<Vec<ToolPlan>, String> {
    let tools = tool_plans(schema, decimal)?;
    if !feature_enabled {
        let declared = mcp_declarations(schema).unwrap_or_default();
        return Err(format!(
            "schema declares an MCP surface ({declared}), but `cratestack-macros` was built \
             without its `mcp` Cargo feature, so nothing would serve it (ADR 0002 Q4). Enable \
             the `mcp` feature on the facade this crate depends on — `cratestack = {{ package = \
             \"cratestack-pg\", features = [\"mcp\"] }}`, or the same on `cratestack-api` — or \
             remove the MCP declarations."
        ));
    }
    if let Some(resources) = resource_declarations(schema) {
        return Err(format!(
            "schema declares MCP resources ({resources}), but MCP resources are not served yet \
             (cratestack#1033, phase 5): include_server_schema! rejects them until they are, so \
             none can parse and then serve nothing (ADR 0002 Q4). Remove `@@mcp(resource: ...)` \
             and `resources` from the `mcp {{ expose = [...] }}` list; tools are served today."
        ));
    }
    Ok(tools)
}

fn tool_plans(schema: &Schema, decimal: Option<DecimalBackend>) -> Result<Vec<ToolPlan>, String> {
    let mut plans = Vec::new();
    for procedure in &schema.procedures {
        let Some(tool) = procedure.mcp.as_ref() else {
            continue;
        };
        let refused = |side: &str, error: JsonSchemaError| {
            format!(
                "`@mcp(tool)` on procedure `{}` cannot be exposed: its {side} has no JSON \
                 Schema. {error}. A type with no faithful JSON Schema mapping is refused rather \
                 than advertised as `{{}}` (ADR 0002 § Tools); remove `@mcp(tool)`, or change \
                 the {side} type.",
                procedure.name
            )
        };
        let input = procedure_input_schema(schema, procedure, decimal)
            .map_err(|error| refused("input", error))?;
        let output = procedure_output_schema(schema, procedure, decimal)
            .map_err(|error| refused("output", error))?;
        plans.push(ToolPlan {
            procedure: procedure.clone(),
            name: tool.tool_name.clone(),
            description: tool.description.clone(),
            input: input.to_string(),
            output: output.map(|value| value.to_string()),
        });
    }
    Ok(plans)
}

/// A short, human list of what the schema declares, or `None` when it
/// declares no MCP at all. Checks all three IR slots rather than only the
/// block: validation already guarantees an attribute implies a block, but
/// this gate is what keeps MCP from being inert, so it does not lean on that.
pub(super) fn mcp_declarations(schema: &Schema) -> Option<String> {
    let mut declared = Vec::new();
    if schema.mcp.is_some() {
        declared.push("an `mcp { }` block".to_owned());
    }
    for procedure in &schema.procedures {
        if procedure.mcp.is_some() {
            declared.push(format!("`@mcp(tool)` on procedure `{}`", procedure.name));
        }
    }
    declared.extend(resource_list(schema));
    (!declared.is_empty()).then(|| declared.join(", "))
}

fn resource_declarations(schema: &Schema) -> Option<String> {
    let mut declared = resource_list(schema);
    if schema
        .mcp
        .as_ref()
        .is_some_and(|mcp| mcp.exposes_resources())
    {
        declared.insert(0, "`resources` in `expose`".to_owned());
    }
    (!declared.is_empty()).then(|| declared.join(", "))
}

fn resource_list(schema: &Schema) -> Vec<String> {
    schema
        .models
        .iter()
        .filter(|model| model.mcp.is_some())
        .map(|model| format!("`@@mcp(resource: ...)` on model `{}`", model.name))
        .collect()
}
