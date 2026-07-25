//! `@pb(N)` extraction from an already-parsed [`Field`].
//!
//! `cratestack-parser` validates the attribute's *shape* (single, numeric,
//! outside the protobuf-reserved range) at schema-load time, but tests in
//! this crate build `cratestack_core::Schema` values directly, bypassing
//! the parser — so this stays defensive rather than assuming its input was
//! already validated.

use cratestack_core::Field;

use super::PbLockError;

pub(super) fn pb_pin(owner: &str, field: &Field) -> Result<Option<i32>, PbLockError> {
    let Some(attribute) = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw == "@pb" || attribute.raw.starts_with("@pb("))
    else {
        return Ok(None);
    };
    parse_pb_number(&attribute.raw)
        .map(Some)
        .map_err(|reason| PbLockError::InvalidPin {
            owner: owner.to_owned(),
            field: field.name.clone(),
            raw: attribute.raw.clone(),
            reason,
        })
}

fn parse_pb_number(raw: &str) -> Result<i32, String> {
    let inner = raw
        .strip_prefix("@pb(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| "expected `@pb(<non-negative integer>)`".to_owned())?;
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return Err("expected a non-negative integer argument".to_owned());
    }
    trimmed
        .parse::<i32>()
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(|| format!("expected a non-negative integer, got `{trimmed}`"))
}
