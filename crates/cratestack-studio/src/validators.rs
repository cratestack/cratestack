//! Server-side mirror of the cratestack-policy validators.
//!
//! The framework's macro path wires `@email`, `@length`, `@range`,
//! `@regex`, `@uri`, and `@iso4217` into generated `validate` impls
//! on Create/Update inputs. Studio doesn't have a typed input shape
//! to lean on (the schema is parsed at runtime), so we re-implement
//! the same predicates here against `serde_json::Value` payloads.
//! Errors are structured per-field so the UI can surface them inline.

mod predicates;
mod types;

#[cfg(test)]
mod predicate_tests;
#[cfg(test)]
mod tests;

use cratestack_core::{Field, Model};

pub use types::{FieldError, ValidationCode};

/// Validate one record's worth of input against the model.
///
/// `payload` is the proposed JSON object. `partial` is `true` for
/// UPDATE flows (missing keys are OK; only present keys get checked).
/// Returns all per-field errors at once — the UI can surface them
/// together rather than failing on the first one.
pub fn validate_payload(
    model: &Model,
    payload: &serde_json::Map<String, serde_json::Value>,
    partial: bool,
) -> Vec<FieldError> {
    let mut errors = Vec::new();
    for field in &model.fields {
        if !is_writable_field(field) {
            continue;
        }
        match payload.get(&field.name) {
            None => {
                if !partial && field_is_required(field) {
                    errors.push(FieldError {
                        field: field.name.clone(),
                        code: ValidationCode::Required,
                        message: format!("field '{}' is required", field.name),
                    });
                }
            }
            Some(serde_json::Value::Null) => {
                if !field_is_optional(field) {
                    errors.push(FieldError {
                        field: field.name.clone(),
                        code: ValidationCode::Required,
                        message: format!("field '{}' must not be null", field.name),
                    });
                }
            }
            Some(v) => {
                if let Some(err) = check_type(field, v) {
                    errors.push(err);
                    continue;
                }
                for attr in &field.attributes {
                    if let Some(err) = predicates::run_attribute(field, &attr.raw, v) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

fn field_is_required(field: &Field) -> bool {
    matches!(field.ty.arity, cratestack_core::TypeArity::Required)
        && !has_default(field)
        && !has_attr(field, "@id")
}

fn field_is_optional(field: &Field) -> bool {
    matches!(field.ty.arity, cratestack_core::TypeArity::Optional)
}

fn has_default(field: &Field) -> bool {
    field.attributes.iter().any(|a| a.raw.starts_with("@default"))
}

fn has_attr(field: &Field, name: &str) -> bool {
    field
        .attributes
        .iter()
        .any(|a| a.raw == name || a.raw.starts_with(&format!("{name}(")))
}

/// Phase 3 writes only the field set that's scalar and not a
/// relation. Detect relation-shaped fields by an `@relation(...)`
/// attribute — the canonical marker the CrateStack parser enforces.
fn is_writable_field(field: &Field) -> bool {
    if matches!(field.ty.arity, cratestack_core::TypeArity::List) {
        return false;
    }
    !field.attributes.iter().any(|a| a.raw.starts_with("@relation"))
}

fn check_type(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let scalar = field.ty.name.as_str();
    let ok = match scalar {
        "String" | "Cuid" | "Uuid" | "DateTime" | "Decimal" | "Bytes" => value.is_string(),
        "Int" => value.is_i64() || value.is_u64(),
        "Float" => value.is_number(),
        "Boolean" => value.is_boolean(),
        "Json" => true,
        // Unknown scalars (enums declared in the schema) — accept
        // strings; the DB layer enforces the enum check.
        _ => value.is_string(),
    };
    if ok {
        return None;
    }
    Some(FieldError {
        field: field.name.clone(),
        code: ValidationCode::TypeMismatch,
        message: format!(
            "field '{}' expected {scalar}; got {}",
            field.name,
            jtype_name(value)
        ),
    })
}

fn jtype_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
