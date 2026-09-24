//! An exposed declaration nobody could ever use is a declaration mistake.
//!
//! Neither rule closes a hole — access is deny-by-default everywhere — but a
//! resource whose reads compile to `FALSE`, or a tool that always refuses,
//! would be advertised to every agent and never work (ADR 0002 § Validation,
//! Security requirement 5).

use cratestack_core::{Model, Procedure, Schema};

use super::{resources, tools};
use crate::diagnostics::{SchemaError, span_error};

/// The action of an `@@allow("<action>", ...)` attribute, read the way the
/// read-policy generator reads it (`cratestack-macros/src/policy/model.rs`,
/// `parse_rule_action`): the first argument, a `"`- or `'`-quoted string,
/// compared exactly — no multi-action lists.
fn allow_action(raw: &str) -> Option<&str> {
    let inner = raw
        .trim()
        .strip_prefix("@@allow")?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    let quote = inner.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let rest = &inner[quote.len_utf8()..];
    rest.find(quote).map(|end| &rest[..end])
}

/// Generated list reads admit `list`/`read`/`all` rules and by-id reads
/// admit `detail`/`read`/`all` (`cratestack-macros/src/model/descriptor.rs`),
/// so a resource — which serves both — needs `read` or `all`, or one `list`
/// and one `detail` allow. `@@deny` never grants anything.
fn has_read_allow(model: &Model) -> bool {
    let actions = model
        .attributes
        .iter()
        .filter_map(|attribute| allow_action(&attribute.raw))
        .collect::<Vec<_>>();
    actions
        .iter()
        .any(|action| matches!(*action, "read" | "all"))
        || (actions.contains(&"list") && actions.contains(&"detail"))
}

/// A procedure is authorized only by an `@allow(...)`: with none, the
/// generated policy always refuses (`cratestack-policy/src/eval.rs`), and
/// `@deny` cannot change that. Read the way
/// `cratestack-macros/src/policy/procedure.rs` reads it.
fn has_allow(procedure: &Procedure) -> bool {
    procedure.attributes.iter().any(|attribute| {
        attribute
            .raw
            .trim()
            .strip_prefix("@allow")
            .is_some_and(|rest| rest.starts_with('('))
    })
}

pub(super) fn resources_need_a_read_allow(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (model, resource) in resources(schema) {
        if !has_read_allow(model) {
            errors.push(span_error(
                format!(
                    "`@@mcp(resource: \"{}\")` on model `{}` exposes a model with no read allow: \
                     add `@@allow(\"read\", ...)` (or `\"all\"`, or both a `\"list\"` and a \
                     `\"detail\"` allow). With none, every read compiles to FALSE and the \
                     resource could never return anything (ADR 0002 § Validation)",
                    resource.resource, model.name
                ),
                resource.span,
            ));
        }
    }
}

pub(super) fn tools_need_an_allow(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for (procedure, tool) in tools(schema) {
        if !has_allow(procedure) {
            errors.push(span_error(
                format!(
                    "`@mcp(tool)` on procedure `{}` exposes a procedure with no `@allow(...)`: \
                     a procedure with no allow policy is always refused, and `@deny` alone \
                     cannot change that, so the tool could never succeed (ADR 0002 § \
                     Validation)",
                    procedure.name
                ),
                tool.span,
            ));
        }
    }
}
