//! `@mcp(tool ...)` on a procedure and `@@mcp(resource: ...)` on a model:
//! the *shape* of each attribute (ADR 0002 § Validation, first two bullets).
//!
//! Only syntax is checked here — required key present, each value the right
//! kind of literal, no unknown or repeated key. Whether a name, segment or
//! page size is *well-formed*, and every rule that needs another declaration
//! to decide, lives in `validate::mcp`, so a defaulted tool name goes through
//! exactly the same format check as a written one.

use cratestack_core::{Attribute, ModelMcpExposure, ProcedureMcpExposure};

use super::args::{page_size, parse_mcp_args, quoted};
use super::position::{MCP, MODEL_MCP, attribute_has_name, reject_embedded_mcp};
use crate::diagnostics::{SchemaError, span_error};

/// Pulls the procedure's `@mcp(...)` out of its raw attribute list, so the
/// typed field is the only copy (see `cratestack_core::schema::mcp`).
pub(crate) fn extract_procedure_mcp(
    procedure: &str,
    attributes: Vec<Attribute>,
) -> Result<(Option<ProcedureMcpExposure>, Vec<Attribute>), SchemaError> {
    let owner = format!("procedure `{procedure}`");
    let mut exposure: Option<ProcedureMcpExposure> = None;
    let mut retained = Vec::new();
    for attribute in attributes {
        if attribute_has_name(&attribute.raw, MODEL_MCP) {
            return Err(span_error(
                format!(
                    "`@@mcp` on {owner}: `@@mcp(resource: ...)` exposes a model; a procedure is \
                     exposed as an MCP tool with `@mcp(tool)` or `@mcp(tool: \"name\")`"
                ),
                attribute.span,
            ));
        }
        if !attribute_has_name(&attribute.raw, MCP) {
            reject_embedded_mcp(&attribute, &owner)?;
            retained.push(attribute);
            continue;
        }
        if exposure.is_some() {
            return Err(span_error(
                format!("{owner} declares `@mcp(...)` more than once"),
                attribute.span,
            ));
        }
        exposure = Some(parse_tool(procedure, &owner, &attribute)?);
    }
    Ok((exposure, retained))
}

/// Pulls the model's `@@mcp(...)` out of its raw attribute list.
pub(crate) fn extract_model_mcp(
    model: &str,
    attributes: Vec<Attribute>,
) -> Result<(Option<ModelMcpExposure>, Vec<Attribute>), SchemaError> {
    let owner = format!("model `{model}`");
    let mut exposure: Option<ModelMcpExposure> = None;
    let mut retained = Vec::new();
    for attribute in attributes {
        if !attribute_has_name(&attribute.raw, MODEL_MCP) {
            reject_embedded_mcp(&attribute, &owner)?;
            retained.push(attribute);
            continue;
        }
        if exposure.is_some() {
            return Err(span_error(
                format!("{owner} declares `@@mcp(...)` more than once"),
                attribute.span,
            ));
        }
        exposure = Some(parse_resource(&owner, &attribute)?);
    }
    Ok((exposure, retained))
}

fn parse_tool(
    procedure: &str,
    owner: &str,
    attribute: &Attribute,
) -> Result<ProcedureMcpExposure, SchemaError> {
    let fail = |detail: String| span_error(format!("`@mcp` on {owner} {detail}"), attribute.span);
    let inner = arguments(&attribute.raw, MCP, "tool").map_err(&fail)?;
    let mut tool: Option<Option<String>> = None;
    let mut description = None;
    for arg in parse_mcp_args(inner).map_err(&fail)? {
        match (arg.key, arg.value) {
            ("tool", _) if tool.is_some() => return Err(fail("repeats `tool`".to_owned())),
            ("tool", None) => tool = Some(None),
            ("tool", Some(value)) => tool = Some(Some(quoted(value, "tool").map_err(&fail)?)),
            ("description", _) if description.is_some() => {
                return Err(fail("repeats `description`".to_owned()));
            }
            ("description", value) => {
                let value = value.unwrap_or_default();
                description = Some(quoted(value, "description").map_err(&fail)?);
            }
            ("resource" | "max_page_size", _) => {
                return Err(fail(format!(
                    "uses `{}:`, which belongs on a model's `@@mcp(resource: \"...\")`; a \
                     procedure takes `tool` and an optional `description:`",
                    arg.key
                )));
            }
            (other, _) => {
                return Err(fail(format!(
                    "has unknown argument `{other}` (expected `tool` and an optional \
                     `description:`)"
                )));
            }
        }
    }
    let Some(tool) = tool else {
        return Err(fail(
            "must contain `tool`, either bare (`@mcp(tool)`) or named (`@mcp(tool: \"name\")`)"
                .to_owned(),
        ));
    };
    Ok(ProcedureMcpExposure {
        tool_name_defaulted: tool.is_none(),
        tool_name: tool.unwrap_or_else(|| procedure.to_owned()),
        description,
        span: attribute.span,
    })
}

fn parse_resource(owner: &str, attribute: &Attribute) -> Result<ModelMcpExposure, SchemaError> {
    let fail = |detail: String| span_error(format!("`@@mcp` on {owner} {detail}"), attribute.span);
    let inner = arguments(&attribute.raw, MODEL_MCP, "resource: \"segment\"").map_err(&fail)?;
    let mut resource = None;
    let mut max_page_size = None;
    for arg in parse_mcp_args(inner).map_err(&fail)? {
        match arg.key {
            "resource" if resource.is_some() => {
                return Err(fail("repeats `resource`".to_owned()));
            }
            "resource" => {
                let value = arg.value.unwrap_or_default();
                resource = Some(quoted(value, "resource").map_err(&fail)?);
            }
            "max_page_size" if max_page_size.is_some() => {
                return Err(fail("repeats `max_page_size`".to_owned()));
            }
            "max_page_size" => max_page_size = Some(page_size(arg.value).map_err(&fail)?),
            "tool" | "description" => {
                return Err(fail(format!(
                    "uses `{}`, which belongs on a procedure's `@mcp(tool ...)`; a model takes \
                     `resource: \"segment\"` and an optional `max_page_size:`",
                    arg.key
                )));
            }
            other => {
                return Err(fail(format!(
                    "has unknown argument `{other}` (expected `resource:` and an optional \
                     `max_page_size:`)"
                )));
            }
        }
    }
    let resource =
        resource.ok_or_else(|| fail("must contain `resource: \"segment\"`".to_owned()))?;
    Ok(ModelMcpExposure {
        resource,
        max_page_size,
        span: attribute.span,
    })
}

/// The text between the parentheses. `example` completes the "write
/// `@mcp(...)`" hint for the attribute at hand.
fn arguments<'a>(raw: &'a str, name: &str, example: &str) -> Result<&'a str, String> {
    let rest = &raw[name.len()..];
    if rest.starts_with('.') {
        return Err(format!(
            "uses the dotted form `{raw}`, which is not MCP syntax: write `{name}({example})` \
             (ADR 0002 D1 chose the argument form over the dotted one)"
        ));
    }
    rest.strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| format!("must be written `{name}({example})`, not `{raw}`"))
}
