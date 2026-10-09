//! `@default(<number>)` on a `BigInt` field (ADR 0019).

use crate::diagnostics::{SchemaError, span_error};

/// A numeric `@default` on a `BigInt` field must be a decimal integer inside
/// `i64`.
///
/// The literal is emitted verbatim as the column's `DEFAULT`
/// (`cratestack-migrate`'s `convert/fields.rs`, `field_default`), so without
/// this check `@default(9223372036854775808)` parses and only surfaces when
/// the migration is applied, far from the schema line. Anything that is not
/// number-shaped (`autoincrement()`, `dbgenerated()`, `auth().id`, a quoted
/// literal) is left to the validators that own it, exactly as on every other
/// scalar. Other scalars are not touched: `Int` is still `i64` in this
/// release and its defaults stay unchecked.
pub(super) fn check_bigint_default(
    model_name: &str,
    field: &cratestack_core::Field,
    scalar: &str,
    raw: &str,
) -> Result<(), SchemaError> {
    if scalar != "BigInt" {
        return Ok(());
    }
    let Some(literal) = raw
        .strip_prefix("@default(")
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::trim)
    else {
        return Ok(());
    };
    let digits = literal.strip_prefix(['+', '-']).unwrap_or(literal);
    if !digits.starts_with(|c: char| c.is_ascii_digit()) || literal.parse::<i64>().is_ok() {
        return Ok(());
    }
    Err(span_error(
        format!(
            "field `{}.{}` @default({literal}) must be a decimal integer inside the `BigInt` \
             range {}..={}",
            model_name,
            field.name,
            i64::MIN,
            i64::MAX,
        ),
        field.span,
    ))
}
