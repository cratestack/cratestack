//! Validation for the `@authn` procedure attribute (ADR 0022,
//! cratestack#1244): read with the one reader the generated enforcement
//! uses (`cratestack_core::parse_authn_attribute`), at most once.
//!
//! A requirement the generator could not read would be a requirement the
//! server does not enforce, so anything it does not read is refused here.

use cratestack_core::schema::{AUTHN_ATTRIBUTE, parse_authn_attribute};
use cratestack_core::{Attribute, Procedure};

use crate::diagnostics::{SchemaError, span_error};

#[cfg(test)]
#[path = "authn_attribute_tests.rs"]
mod tests;

fn is_authn(attribute: &Attribute) -> bool {
    attribute
        .raw
        .trim()
        .strip_prefix(AUTHN_ATTRIBUTE)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// `@authn` reads, and appears at most once.
pub(super) fn validate_procedure_authn_attribute(procedure: &Procedure) -> Result<(), SchemaError> {
    let found: Vec<&Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| is_authn(a))
        .collect();
    if let Some(repeat) = found.get(1) {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @authn attribute; put every requirement in one",
                procedure.name
            ),
            repeat.span,
        ));
    }
    if let Some(attribute) = found.first() {
        parse_authn_attribute(&attribute.raw).map_err(|reason| {
            span_error(
                format!("procedure `{}`: {reason}", procedure.name),
                attribute.span,
            )
        })?;
    }
    Ok(())
}
