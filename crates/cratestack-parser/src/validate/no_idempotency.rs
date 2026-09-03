//! Validation for the bare `@no_idempotency` procedure attribute (#876).
//!
//! Mirrors `validate_procedure_no_rate_limit_attribute`
//! (`validate/procedures.rs`) — reject arguments, reject duplicates —
//! **minus the `extension` gate**, and that omission is deliberate rather
//! than an oversight. `@no_rate_limit` is only meaningful in a schema that
//! declared `extension rate_limit { }`, because declaring the extension is
//! what unlocks the attribute (layer 1 of the extension model,
//! `docs/design/extensions.md` §2). `@no_idempotency` has never had such a
//! block, and inventing one here would be adding grammar to satisfy a
//! symmetry nobody asked for; whether idempotency should acquire an
//! `extension idempotency { }` is an epic-level question (#875).
//!
//! Lives in its own file rather than joining `validate/procedures.rs`
//! because that file is already on `.ci/file-length-allowlist.toml`, and
//! that list is a paydown list — it may shrink, not grow.

#[cfg(test)]
mod tests;

use crate::diagnostics::{SchemaError, span_error};

/// `@no_idempotency` takes no arguments and may appear at most once.
///
/// Both halves matter because the attribute reads like a toggle. Before
/// this validator existed, `@no_idempotency(true)` passed `cratestack-cli
/// check` with `schema OK` and then emitted `idempotent_by_default:
/// false` — i.e. the argument was accepted and silently inverted the
/// author's evident intent. Rejecting the form outright is the only
/// answer that cannot be misread: there is no argument this attribute
/// could take, so anything in the parentheses is a mistake.
pub(super) fn validate_procedure_no_idempotency_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw == "@no_idempotency" || a.raw.starts_with("@no_idempotency("))
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @no_idempotency attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    if attr.raw != "@no_idempotency" {
        return Err(span_error(
            format!(
                "procedure `{}` @no_idempotency does not take any arguments — it is a bare \
                 opt-out marker, and a value like `@no_idempotency(false)` would read as \
                 re-enabling idempotency while doing the opposite",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}
