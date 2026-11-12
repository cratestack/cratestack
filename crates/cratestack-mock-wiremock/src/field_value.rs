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
    Field, Schema, TypeArity, validate_email, validate_iso4217, validate_length, validate_uri,
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
        "String" => Value::String(string_value(field, &spec).0),
        "Bytes" => bytes_value(&spec),
        _ => int_value(&spec, scalar == "BigInt"),
    };
    Ok(match field.ty.arity {
        TypeArity::List => Value::Array(vec![one]),
        _ => one,
    })
}

/// Zero clamped into the `@range`: the lower bound for a positive-only
/// range, the upper bound for a negative-only one. The parser guarantees
/// `min <= max`, so the result always satisfies the range.
fn int_value(spec: &ValidatorSpec, big: bool) -> Value {
    let (min, max) = spec.range;
    let n = 0i64
        .max(min.unwrap_or(i64::MIN))
        .min(max.unwrap_or(i64::MAX));
    if big { json!(n.to_string()) } else { json!(n) }
}

/// `@length(min)` zero bytes (the parser guarantees `min <= max`).
fn bytes_value(spec: &ValidatorSpec) -> Value {
    json!(vec![0u8; spec.length.0.unwrap_or(0)])
}

/// The `String` placeholder plus the raw validator attributes it still
/// fails (empty when it satisfies all of them). When no candidate
/// satisfies everything the best one wins: fewest failures, ties to the
/// first (a `@regex` sample before the plain fallback).
pub(crate) fn string_value(field: &Field, spec: &ValidatorSpec) -> (String, Vec<String>) {
    let (min, max) = spec.length;
    let mut candidates: Vec<String> = Vec::new();
    if let Some(pattern) = spec.regexes.first() {
        // Lengthen unbounded repetitions until `@length`'s minimum is met.
        candidates
            .extend((0..=min.unwrap_or(0).min(256) as u32).filter_map(|e| sample(pattern, e)));
    }
    if spec.iso4217 {
        candidates.push("EUR".to_owned());
    } else if spec.email {
        candidates.push("user@example.com".to_owned());
    } else if spec.uri {
        candidates.push("https://example.com".to_owned());
    }
    let mut text = "string".to_owned();
    while text.chars().count() < min.unwrap_or(0) {
        text.push('a');
    }
    candidates.push(text.chars().take(max.unwrap_or(usize::MAX)).collect());
    candidates
        .into_iter()
        .map(|c| {
            let failed = failures(field, spec, &c);
            (c, failed)
        })
        .reduce(|best, next| {
            if next.1.len() < best.1.len() {
                next
            } else {
                best
            }
        })
        .expect("the plain fallback candidate is always present")
}

/// The field's raw validator attributes that `value` violates, judged by
/// the same checks the generated server code runs.
fn failures(field: &Field, spec: &ValidatorSpec, value: &str) -> Vec<String> {
    let (min, max) = spec.length;
    field
        .attributes
        .iter()
        .filter(|attribute| {
            let raw = attribute.raw.as_str();
            let ok = match raw.split('(').next().unwrap_or(raw) {
                "@length" => validate_length("f", value, min, max).is_ok(),
                "@email" => validate_email("f", value).is_ok(),
                "@uri" => validate_uri("f", value).is_ok(),
                "@iso4217" => validate_iso4217("f", value).is_ok(),
                "@regex" => spec
                    .regex_for(raw)
                    .is_none_or(|p| regex::Regex::new(p).is_ok_and(|re| re.is_match(value))),
                _ => true,
            };
            !ok
        })
        .map(|attribute| attribute.raw.clone())
        .collect()
}
