//! Field-level placeholder synthesis that honours the field's own
//! validators (`@range`, `@length`, `@regex`, `@email`, `@uri`,
//! `@iso4217`), so a stub never carries a value the real server's
//! generated `validate` would reject (cratestack#1307).
//!
//! Every candidate is checked with the same `cratestack_core::validate_*`
//! helpers (and the same `regex` crate) the generated code calls. A field
//! with no validators, or one of a type no validator applies to, takes the
//! unchanged [`crate::values::synthesize`] path.

use cratestack_core::{
    Field, Schema, TypeArity, validate_email, validate_iso4217, validate_length,
    validate_length_bytes, validate_range_i64, validate_uri,
};
use serde_json::{Value, json};

use crate::error::WireMockGeneratorError;
use crate::regex_sample::sample;
use crate::validator_spec::ValidatorSpec;
use crate::values::synthesize;

pub(crate) fn synthesize_field(
    schema: &Schema,
    owner: &str,
    field: &Field,
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    let spec = ValidatorSpec::from_field(field);
    let scalar = field.ty.name.as_str();
    if spec.is_empty() || !matches!(scalar, "String" | "Bytes" | "Int" | "BigInt") {
        return synthesize(schema, owner, &field.ty, in_progress);
    }
    let one = match scalar {
        "String" => string_value(&spec),
        "Bytes" => bytes_value(&spec),
        _ => int_value(&spec, scalar == "BigInt"),
    }
    .ok_or_else(|| WireMockGeneratorError::UnsatisfiableValidators {
        owner: owner.to_owned(),
        field: field.name.clone(),
    })?;
    Ok(match field.ty.arity {
        TypeArity::List => Value::Array(vec![one]),
        _ => one,
    })
}

/// Zero clamped into the `@range`: the lower bound for a positive-only
/// range, the upper bound for a negative-only one.
fn int_value(spec: &ValidatorSpec, big: bool) -> Option<Value> {
    let (min, max) = spec.range;
    let n = 0i64
        .max(min.unwrap_or(i64::MIN))
        .min(max.unwrap_or(i64::MAX));
    validate_range_i64("f", n, min, max).ok()?;
    Some(if big { json!(n.to_string()) } else { json!(n) })
}

fn bytes_value(spec: &ValidatorSpec) -> Option<Value> {
    let (min, max) = spec.length;
    let bytes = vec![0u8; min.unwrap_or(0)];
    validate_length_bytes("f", &bytes, min, max).ok()?;
    Some(json!(bytes))
}

fn string_value(spec: &ValidatorSpec) -> Option<Value> {
    let (min, max) = spec.length;
    let mut candidates: Vec<String> = Vec::new();
    if let Some(pattern) = spec.regexes.first() {
        // Lengthen unbounded repetitions until `@length`'s minimum is met.
        candidates
            .extend((0..=min.unwrap_or(0).min(256) as u32).filter_map(|e| sample(pattern, e)));
    } else if spec.iso4217 {
        candidates.push("EUR".to_owned());
    } else if spec.email {
        candidates.push("user@example.com".to_owned());
    } else if spec.uri {
        candidates.push("https://example.com".to_owned());
    } else {
        let mut text = "string".to_owned();
        while text.chars().count() < min.unwrap_or(0) {
            text.push('a');
        }
        candidates.push(text.chars().take(max.unwrap_or(usize::MAX)).collect());
    }
    candidates
        .into_iter()
        .find(|c| satisfies(spec, c))
        .map(Value::String)
}

fn satisfies(spec: &ValidatorSpec, value: &str) -> bool {
    let (min, max) = spec.length;
    validate_length("f", value, min, max).is_ok()
        && (!spec.email || validate_email("f", value).is_ok())
        && (!spec.uri || validate_uri("f", value).is_ok())
        && (!spec.iso4217 || validate_iso4217("f", value).is_ok())
        && spec
            .regexes
            .iter()
            .all(|p| regex::Regex::new(p).is_ok_and(|re| re.is_match(value)))
}
