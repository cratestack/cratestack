//! Does the `mcp { }` block have a `name` exactly when it needs one? The
//! name is the `<name>` of every resource URI,
//! `cratestack://<name>/<segment>/{id}` (maintainer decision on
//! cratestack#1040); its shape is checked where it is parsed
//! (`parse::mcp::name`).

use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};

/// Resources are addressed by the name, so exposing them without one leaves
/// every URI without a host. Points at the `resources` element, the part of
/// the block that asks for a name.
pub(super) fn resources_need_a_name(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let Some(config) = &schema.mcp else {
        return;
    };
    if let (Some(resources), None) = (config.expose_resources, &config.name) {
        errors.push(span_error(
            "mcp block: `expose` lists `resources`, but the block has no `name`: resource URIs \
             are `cratestack://<name>/<segment>/{id}`, so add `name = \"...\"` (lowercase \
             letters, digits and `-`), for example `name = \"blog\"`",
            resources,
        ));
    }
}

/// A `name` without resources is refused, not allowed. Nothing but a
/// resource URI reads it, so it would be an inert declaration, the failure
/// mode `cratestack_core::schema::mcp`'s module doc says the MCP IR exists
/// to rule out. Refusing it also keeps the choice open: if the name comes
/// to mean something for tools too, allowing it then breaks no schema,
/// while refusing it after it had been allowed would.
pub(super) fn name_needs_resources(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let Some(config) = &schema.mcp else {
        return;
    };
    if let (None, Some(name)) = (config.expose_resources, &config.name) {
        errors.push(span_error(
            "mcp block: `name` is the `<name>` of resource URIs, and `expose` does not list \
             `resources`, so it would name nothing: remove `name`, or add `resources` to \
             `expose`",
            name.span,
        ));
    }
}
