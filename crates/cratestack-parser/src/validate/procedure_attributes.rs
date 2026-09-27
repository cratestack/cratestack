//! The closed list of `procedure` attributes (GHSA-69g4-xvcm-vm2j).
//!
//! Every name below was derived from its reader (paths under `crates/`);
//! nothing else reads a procedure attribute, so any other name had no
//! effect at all, and a misspelled policy attribute left the procedure
//! more permissive than written.
//!
//! | Attribute         | Arguments | Reader |
//! |-------------------|-----------|--------|
//! | `@allow`          | required  | `cratestack-macros/src/policy/procedure.rs` (`parse_procedure_allow_expression`) |
//! | `@deny`           | required  | same (`parse_procedure_deny_expression`) |
//! | `@authorize`      | required  | `cratestack-macros/src/procedure/authorizer.rs` |
//! | `@api_version`    | required  | `cratestack-macros/src/axum/procedure/route_attrs.rs` (`procedure_api_version`) |
//! | `@status`         | required  | same (`procedure_success_status_tokens`) |
//! | `@deprecated`     | optional  | same (`procedure_deprecation_header_tokens`) |
//! | `@stream`         | none      | `cratestack-macros/src/shared/procedure_attrs.rs` |
//! | `@no_idempotency` | none      | `cratestack-macros/src/transport/idempotency.rs` |
//! | `@no_rate_limit`  | none      | `cratestack-macros/src/transport/rate_limit.rs` |
//! | `@isolation`      | required  | validated by `super::procedures` only: no generator reads it yet |
//! | `@mcp`            | required  | `crate::parse::mcp` (moved to `Procedure::mcp` while parsing) |
//!
//! Runs after the per-attribute validators, so their more specific
//! messages (`@isolation requires a quoted level argument`, …) still win
//! where they apply.

use cratestack_core::Procedure;

use super::attribute_shape::{Arguments, Known, check_shape};
use crate::diagnostics::{SchemaError, span_error};

const PROCEDURE_ATTRIBUTES: &[Known] = &[
    ("@allow", Arguments::Required),
    ("@deny", Arguments::Required),
    ("@authorize", Arguments::Required),
    ("@api_version", Arguments::Required),
    ("@status", Arguments::Required),
    ("@deprecated", Arguments::Optional),
    ("@stream", Arguments::None),
    ("@no_idempotency", Arguments::None),
    ("@no_rate_limit", Arguments::None),
    ("@isolation", Arguments::Required),
    ("@mcp", Arguments::Required),
];

/// The actions `@authorize(Model, action, args.path)` supports
/// (`generate_procedure_model_authorizer`).
const AUTHORIZE_ACTIONS: &[&str] = &["detail", "read", "update", "delete"];

pub(super) fn validate_procedure_attributes(procedure: &Procedure) -> Result<(), SchemaError> {
    let owner = format!("procedure `{}`", procedure.name);
    for attribute in &procedure.attributes {
        let (name, inner) = check_shape(attribute, PROCEDURE_ATTRIBUTES, &owner, "procedure")?;
        if name != "@authorize" {
            continue;
        }
        let parts = inner
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>();
        let action = parts.get(1).map(|action| action.trim_matches(['"', '\'']));
        if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
            return Err(span_error(
                format!(
                    "{owner} writes `{}`: `@authorize` takes exactly three arguments, \
                     `@authorize(Model, action, args.path)`",
                    attribute.raw
                ),
                attribute.span,
            ));
        }
        if let Some(action) = action.filter(|action| !AUTHORIZE_ACTIONS.contains(action)) {
            return Err(span_error(
                format!(
                    "{owner} writes `{}`: `@authorize` supports the actions {}, not `{action}`",
                    attribute.raw,
                    AUTHORIZE_ACTIONS.join(", ")
                ),
                attribute.span,
            ));
        }
    }
    Ok(())
}
