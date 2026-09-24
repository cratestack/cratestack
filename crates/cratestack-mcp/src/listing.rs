//! Turns the generated [`ToolDescriptor`] table into `rmcp` [`Tool`]s, once,
//! when the server is built.
//!
//! Parsing at construction rather than on every `tools/list` means a table
//! whose schemas are not JSON objects fails at startup, where the operator
//! sees it, instead of on an agent's first call. The generator emits them
//! from `serde_json::Value::to_string`, so this is a guard against a
//! hand-written table, not an expected path.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};

use crate::table::ToolDescriptor;

/// A tool table this crate cannot serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTableError {
    tool: &'static str,
    reason: String,
}

impl fmt::Display for ToolTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "mcp tool `{}`: {}", self.tool, self.reason)
    }
}

impl std::error::Error for ToolTableError {}

pub(crate) fn build_listing(descriptors: &[ToolDescriptor]) -> Result<Vec<Tool>, ToolTableError> {
    let mut tools = Vec::with_capacity(descriptors.len());
    for (index, descriptor) in descriptors.iter().enumerate() {
        if descriptors[..index]
            .iter()
            .any(|earlier| earlier.name == descriptor.name)
        {
            // The parser rejects duplicate tool names; a duplicate here would
            // make `tools/call` ambiguous, so it is refused, not deduplicated.
            return Err(error(descriptor, "the name appears twice in the table"));
        }
        let input = object(descriptor, "input", descriptor.input_schema)?;
        let mut tool = Tool::new_with_raw(
            descriptor.name,
            descriptor.description.map(Cow::Borrowed),
            input,
        )
        .with_annotations(annotations(descriptor));
        if let Some(output) = descriptor.output_schema {
            tool = tool.with_raw_output_schema(object(descriptor, "output", output)?);
        }
        tools.push(tool);
    }
    Ok(tools)
}

/// ADR 0002 § Tools: `procedure` → `readOnlyHint: true`; `mutation
/// procedure` → `readOnlyHint: false` plus `idempotentHint` from
/// `OpDescriptor.idempotent_by_default`. The spec gives `idempotentHint`
/// meaning only when `readOnlyHint` is false, so a read carries none.
/// Hints only: clients must not trust them, and nothing here relies on them.
fn annotations(descriptor: &ToolDescriptor) -> ToolAnnotations {
    let annotations = ToolAnnotations::new().read_only(descriptor.read_only);
    if descriptor.read_only {
        annotations
    } else {
        annotations.idempotent(descriptor.op.idempotent_by_default)
    }
}

fn object(
    descriptor: &ToolDescriptor,
    which: &str,
    schema: &str,
) -> Result<Arc<JsonObject>, ToolTableError> {
    match serde_json::from_str::<serde_json::Value>(schema) {
        Ok(serde_json::Value::Object(object)) => Ok(Arc::new(object)),
        Ok(_) => Err(error(
            descriptor,
            format!("the {which} schema is not a JSON object"),
        )),
        Err(parse) => Err(error(
            descriptor,
            format!("the {which} schema is not JSON: {parse}"),
        )),
    }
}

fn error(descriptor: &ToolDescriptor, reason: impl Into<String>) -> ToolTableError {
    ToolTableError {
        tool: descriptor.name,
        reason: reason.into(),
    }
}
