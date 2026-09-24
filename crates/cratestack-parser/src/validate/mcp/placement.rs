//! `@mcp`/`@@mcp` anywhere the parser does not extract it.
//!
//! The parser only turns `@mcp` into IR on a procedure and `@@mcp` on a
//! model. Everywhere else the attribute would stay a raw `Attribute` that no
//! code reads — silently inert, which ADR 0002 § Validation rules out for
//! MCP. The other positions are covered elsewhere: a single-`@` directive in
//! a model body is already "unsupported model directive", `@@mcp` on a
//! procedure is rejected while the parser extracts `@mcp`, and a `query`
//! block rejects every attribute but `@@sql`/`@allow`/`@deny`.

use cratestack_core::{Field, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::parse::mcp::position::{MCP, MODEL_MCP, attribute_has_name};

fn is_mcp(raw: &str) -> bool {
    attribute_has_name(raw, MCP) || attribute_has_name(raw, MODEL_MCP)
}

/// Every field-bearing declaration: model, view, mixin, type and the `auth`
/// block — the same five `validate::removed_attributes` documents.
pub(super) fn no_mcp_on_fields(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let owners = schema
        .models
        .iter()
        .map(|model| ("model", &model.name, &model.fields))
        .chain(
            schema
                .views
                .iter()
                .map(|view| ("view", &view.name, &view.fields)),
        )
        .chain(
            schema
                .mixins
                .iter()
                .map(|mixin| ("mixin", &mixin.name, &mixin.fields)),
        )
        .chain(schema.types.iter().map(|ty| ("type", &ty.name, &ty.fields)))
        .chain(
            schema
                .auth
                .iter()
                .map(|auth| ("auth block", &auth.name, &auth.fields)),
        );
    for (kind, owner, fields) in owners {
        for field in fields.iter() {
            check_field(kind, owner, field, errors);
        }
    }
}

fn check_field(kind: &str, owner: &str, field: &Field, errors: &mut Vec<SchemaError>) {
    for attribute in field.attributes.iter().filter(|a| is_mcp(&a.raw)) {
        errors.push(span_error(
            format!(
                "field `{}` on {kind} `{owner}` uses `{}`, but MCP exposure is declared on a \
                 procedure (`@mcp(tool)`) or a model (`@@mcp(resource: ...)`), never on a \
                 field — an MCP attribute anywhere else would be silently ignored (ADR 0002 § \
                 Validation)",
                field.name, attribute.raw
            ),
            attribute.span,
        ));
    }
}

/// A view is not an MCP resource in v1 (ADR 0002 § Decision: procedures as
/// tools, models as read-only resources).
pub(super) fn no_mcp_on_views(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for view in &schema.views {
        for attribute in view.attributes.iter().filter(|a| is_mcp(&a.raw)) {
            errors.push(span_error(
                format!(
                    "view `{}` uses `{}`, but only a model can be an MCP resource: \
                     `@@mcp(resource: ...)` goes on a `model` (ADR 0002 § Schema surface)",
                    view.name, attribute.raw
                ),
                attribute.span,
            ));
        }
    }
}
