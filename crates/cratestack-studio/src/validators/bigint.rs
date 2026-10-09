//! `BigInt` payload checks (ADR 0019).
//!
//! A `BigInt` travels as a canonical decimal string on every wire, never
//! as a JSON number: a JavaScript client rounds anything past 2^53, so a
//! number here may already be a different value than the operator typed.
//! The grammar is `cratestack_core::BigInt`'s own `FromStr`, so Studio
//! and the generated server cannot disagree about what is canonical.

use cratestack_core::{BigInt, Field};

use super::types::{FieldError, ValidationCode};

/// `None` when `value` is a canonical `BigInt` string, else the
/// type-mismatch error naming the field and the reason.
pub(super) fn check(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let reason = match value {
        serde_json::Value::String(text) => match text.parse::<BigInt>() {
            Ok(_) => return None,
            Err(error) => error.to_string(),
        },
        serde_json::Value::Number(_) => {
            "got a JSON number; send the value as a decimal string".to_owned()
        }
        other => format!("got {}", super::jtype_name(other)),
    };
    Some(FieldError {
        field: field.name.clone(),
        code: ValidationCode::TypeMismatch,
        message: format!(
            "field '{}' expected BigInt as a canonical decimal string: {reason}",
            field.name
        ),
    })
}

/// A `BigInt` string's value, for predicates that compare numbers
/// (`@range`). `None` for anything `check` would refuse.
pub(super) fn value_of(value: &serde_json::Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|text| text.parse::<BigInt>().ok())
        .map(BigInt::get)
}
