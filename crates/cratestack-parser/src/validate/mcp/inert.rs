//! MCP text the parser does not read, wherever it survived parsing.
//!
//! The parser turns exactly two spellings into IR: `@mcp(...)` on its own
//! line under a procedure, `@@mcp(...)` on its own line in a model. Both are
//! then *removed* from the raw attribute lists. So after parsing, any raw
//! attribute (or enum variant name) that still names MCP is, by
//! construction, read by nothing — `@MCP(tool)`, `@ mcp(tool)`,
//! `@allow(true) @mcp(tool)` on a query line, `@@allow(...) @@mcp(...)` on a
//! view line, a bare `@@mcp` line in an enum. Each of those parsed cleanly
//! and exposed nothing before this rule, and a schema whose only MCP was
//! such a spelling also slipped past the macro's release gate, which reads
//! the typed IR (review finding on cratestack#1036; ADR 0002 § Validation:
//! never silently inert).
//!
//! One invariant over the whole IR rather than a rule per position, so a new
//! attribute-bearing declaration cannot quietly become a place MCP hides.
//! The exact spellings in a wrong position (a field, a view) keep their more
//! specific messages from `placement`, and a query's own attribute check
//! names them too, so this rule skips them rather than report twice.

use cratestack_core::{Attribute, Field, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::parse::mcp::position::{MCP, MODEL_MCP, attribute_has_name, mentions_mcp};

pub(super) fn no_inert_mcp(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for model in &schema.models {
        let owner = format!("model `{}`", model.name);
        check_attributes(&owner, &model.attributes, errors);
        check_fields(&owner, &model.fields, errors);
    }
    for view in &schema.views {
        let owner = format!("view `{}`", view.name);
        check_attributes(&owner, &view.attributes, errors);
        check_fields(&owner, &view.fields, errors);
    }
    for procedure in &schema.procedures {
        let owner = format!("procedure `{}`", procedure.name);
        check_attributes(&owner, &procedure.attributes, errors);
    }
    for query in &schema.queries {
        let owner = format!("query `{}`", query.name);
        check_attributes(&owner, &query.attributes, errors);
    }
    for mixin in &schema.mixins {
        check_fields(&format!("mixin `{}`", mixin.name), &mixin.fields, errors);
    }
    for ty in &schema.types {
        check_fields(&format!("type `{}`", ty.name), &ty.fields, errors);
    }
    if let Some(auth) = &schema.auth {
        check_fields(&format!("auth block `{}`", auth.name), &auth.fields, errors);
    }
    for decl in &schema.enums {
        let owner = format!("enum `{}`", decl.name);
        for variant in decl.variants.iter().filter(|v| mentions_mcp(&v.name)) {
            errors.push(inert(&variant.name, &owner, variant.span));
        }
    }
}

fn check_fields(owner: &str, fields: &[Field], errors: &mut Vec<SchemaError>) {
    for field in fields {
        let owner = format!("field `{}` on {owner}", field.name);
        check_attributes(&owner, &field.attributes, errors);
    }
}

fn check_attributes(owner: &str, attributes: &[Attribute], errors: &mut Vec<SchemaError>) {
    for attribute in attributes {
        let exact = attribute_has_name(&attribute.raw, MCP)
            || attribute_has_name(&attribute.raw, MODEL_MCP);
        if !exact && mentions_mcp(&attribute.raw) {
            errors.push(inert(&attribute.raw, owner, attribute.span));
        }
    }
}

fn inert(raw: &str, owner: &str, span: cratestack_core::SourceSpan) -> SchemaError {
    span_error(
        format!(
            "`{raw}` on {owner} names MCP but is not an MCP attribute the parser reads, so it \
             would be silently ignored: MCP is declared only as `@mcp(...)` on its own line \
             under a procedure or `@@mcp(...)` on its own line in a model, in lowercase with no \
             space after the `@` (ADR 0002 § Validation)"
        ),
        span,
    )
}
