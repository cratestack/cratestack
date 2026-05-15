//! Field-level validators.
//!
//! Standalone helpers invoked from generated `validate` methods on
//! Create / Update input structs. Each returns `Ok(())` on success or
//! a redacted [`CoolError::Validation`] whose public message names
//! the field but never echoes the rejected value (so PII does not
//! leak via 422 bodies).

use crate::Decimal;
use crate::error::CoolError;

#[cfg(test)]
mod tests;

pub fn validate_length(
    field: &'static str,
    value: &str,
    min: Option<usize>,
    max: Option<usize>,
) -> Result<(), CoolError> {
    let len = value.chars().count();
    if let Some(min) = min
        && len < min
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' length {len} is below minimum {min}",
        )));
    }
    if let Some(max) = max
        && len > max
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' length {len} exceeds maximum {max}",
        )));
    }
    Ok(())
}

pub fn validate_range_i64(
    field: &'static str,
    value: i64,
    min: Option<i64>,
    max: Option<i64>,
) -> Result<(), CoolError> {
    if let Some(min) = min
        && value < min
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' is below minimum {min}",
        )));
    }
    if let Some(max) = max
        && value > max
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' exceeds maximum {max}",
        )));
    }
    Ok(())
}

/// Decimal-typed `@range` enforcement. The parser accepts integer
/// bounds (`@range(min: 0, max: 100)`) on both Int and Decimal
/// fields; the i64 bounds are promoted to Decimal here so monetary
/// fields can declare the same shape as integer counters. Banks
/// routinely write things like `amount Decimal @range(min: 0)` to
/// forbid negative amounts at the framework layer — without this,
/// the validator silently no-ops and out-of-range values reach the
/// database.
pub fn validate_range_decimal(
    field: &'static str,
    value: &Decimal,
    min: Option<i64>,
    max: Option<i64>,
) -> Result<(), CoolError> {
    if let Some(min) = min {
        let bound = Decimal::from(min);
        if *value < bound {
            return Err(CoolError::Validation(format!(
                "field '{field}' is below minimum {min}",
            )));
        }
    }
    if let Some(max) = max {
        let bound = Decimal::from(max);
        if *value > bound {
            return Err(CoolError::Validation(format!(
                "field '{field}' exceeds maximum {max}",
            )));
        }
    }
    Ok(())
}

/// Pragmatic email check: requires exactly one `@`, non-empty local
/// and domain parts, at least one `.` in the domain, and no
/// whitespace. Not a full RFC 5322 grammar — that grammar admits
/// forms (quoted local parts, IP literals) banks rarely accept
/// anyway. Reject early; let real KYC flows do deeper validation.
pub fn validate_email(field: &'static str, value: &str) -> Result<(), CoolError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.chars().filter(|c| *c == '@').count() != 1
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid email address",
        )));
    }
    let (local, domain) = trimmed.split_once('@').unwrap();
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid email address",
        )));
    }
    Ok(())
}

pub fn validate_uri(field: &'static str, value: &str) -> Result<(), CoolError> {
    if url::Url::parse(value).is_err() {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid URI",
        )));
    }
    Ok(())
}

/// ISO 4217 currency codes are 3 ASCII uppercase letters. We do not
/// enforce the registered set here — that table churns and is
/// downstream policy. Banks typically pin allowed currencies via a
/// separate allow-list anyway.
pub fn validate_iso4217(field: &'static str, value: &str) -> Result<(), CoolError> {
    if value.len() != 3 || !value.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(CoolError::Validation(format!(
            "field '{field}' must be a 3-letter uppercase ISO 4217 code",
        )));
    }
    Ok(())
}
