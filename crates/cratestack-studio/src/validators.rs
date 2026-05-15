//! Server-side mirror of the cratestack-policy validators.
//!
//! The framework's macro path wires `@email`, `@length`, `@range`,
//! `@regex`, `@uri`, and `@iso4217` into generated `validate` impls
//! on Create/Update inputs. Studio doesn't have a typed input shape
//! to lean on (the schema is parsed at runtime), so we re-implement
//! the same predicates here against `serde_json::Value` payloads.
//! Errors are structured per-field so the UI can surface them inline.

use cratestack_core::{Field, Model};
use serde::Serialize;

/// Per-field validation failure. The wire shape is stable: code +
/// human-readable message + the field name that failed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub code: ValidationCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationCode {
    /// `null` (or missing key) on a required, non-defaulted field.
    Required,
    /// Value is not a JSON string / number / boolean as expected.
    TypeMismatch,
    /// `@email` rejected the value.
    Email,
    /// `@length(min: …, max: …)` rejected the value.
    Length,
    /// `@range(min: …, max: …)` rejected the value.
    Range,
    /// `@regex("…")` rejected the value.
    Regex,
    /// `@uri` rejected the value.
    Uri,
    /// `@iso4217` rejected the value (currency code).
    Iso4217,
}

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
        if !is_writable_field(model, field) {
            continue;
        }
        let value = payload.get(&field.name);
        match value {
            None => {
                if !partial && field_is_required(field) {
                    errors.push(FieldError {
                        field: field.name.clone(),
                        code: ValidationCode::Required,
                        message: format!("field '{}' is required", field.name),
                    });
                }
                continue;
            }
            Some(serde_json::Value::Null) => {
                if !field_is_optional(field) {
                    errors.push(FieldError {
                        field: field.name.clone(),
                        code: ValidationCode::Required,
                        message: format!("field '{}' must not be null", field.name),
                    });
                }
                continue;
            }
            Some(v) => {
                if let Some(err) = check_type(field, v) {
                    errors.push(err);
                    continue;
                }
                for attr in &field.attributes {
                    if let Some(err) = run_attribute(field, &attr.raw, v) {
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
/// relation. We detect relation-shaped fields by the field carrying
/// an `@relation(...)` attribute — that's the canonical marker the
/// CrateStack parser enforces on every relation field.
fn is_writable_field(_model: &Model, field: &Field) -> bool {
    if matches!(field.ty.arity, cratestack_core::TypeArity::List) {
        return false;
    }
    let has_relation = field
        .attributes
        .iter()
        .any(|a| a.raw.starts_with("@relation"));
    !has_relation
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

fn run_attribute(field: &Field, raw: &str, value: &serde_json::Value) -> Option<FieldError> {
    if raw == "@email" {
        return check_email(field, value);
    }
    if raw == "@uri" {
        return check_uri(field, value);
    }
    if raw == "@iso4217" {
        return check_iso4217(field, value);
    }
    if let Some(body) = strip_call(raw, "@length") {
        return check_length(field, value, body);
    }
    if let Some(body) = strip_call(raw, "@range") {
        return check_range(field, value, body);
    }
    if let Some(body) = strip_call(raw, "@regex") {
        return check_regex(field, value, body);
    }
    None
}

fn strip_call<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
}

fn check_email(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let s = value.as_str()?;
    // Cheap RFC-5322-ish check: one '@', non-empty local + domain,
    // at least one dot in the domain. Same shape as the macro's
    // generated email predicate.
    let bad = s.matches('@').count() != 1
        || !s.contains('.')
        || s.starts_with('@')
        || s.ends_with('@')
        || s.contains(' ');
    if bad {
        Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Email,
            message: format!("field '{}' is not a valid email address", field.name),
        })
    } else {
        None
    }
}

fn check_uri(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let s = value.as_str()?;
    let bad = url::Url::parse(s).is_err();
    if bad {
        Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Uri,
            message: format!("field '{}' is not a valid URI", field.name),
        })
    } else {
        None
    }
}

fn check_iso4217(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let s = value.as_str()?;
    // ISO 4217 is a 3-character uppercase code. The macro path
    // doesn't validate against a closed list because new codes get
    // added; we mirror that.
    let bad = s.len() != 3 || !s.chars().all(|c| c.is_ascii_uppercase());
    if bad {
        Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Iso4217,
            message: format!(
                "field '{}' must be a 3-letter uppercase ISO 4217 currency code",
                field.name
            ),
        })
    } else {
        None
    }
}

fn check_length(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let s = value.as_str()?;
    let len = s.chars().count() as i64;
    let (min, max) = parse_min_max(args);
    if let Some(m) = min
        && len < m
    {
        return Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Length,
            message: format!(
                "field '{}' must be at least {} character{} long",
                field.name,
                m,
                if m == 1 { "" } else { "s" }
            ),
        });
    }
    if let Some(m) = max
        && len > m
    {
        return Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Length,
            message: format!(
                "field '{}' must be at most {} character{} long",
                field.name,
                m,
                if m == 1 { "" } else { "s" }
            ),
        });
    }
    None
}

fn check_range(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let n = value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))?;
    let (min, max) = parse_min_max(args);
    if let Some(m) = min
        && n < m
    {
        return Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Range,
            message: format!("field '{}' must be at least {}", field.name, m),
        });
    }
    if let Some(m) = max
        && n > m
    {
        return Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Range,
            message: format!("field '{}' must be at most {}", field.name, m),
        });
    }
    None
}

fn check_regex(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let s = value.as_str()?;
    let pattern = args.trim().trim_matches('"');
    let regex = regex::Regex::new(pattern).ok()?;
    if !regex.is_match(s) {
        Some(FieldError {
            field: field.name.clone(),
            code: ValidationCode::Regex,
            message: format!("field '{}' did not match the required pattern", field.name),
        })
    } else {
        None
    }
}

fn parse_min_max(args: &str) -> (Option<i64>, Option<i64>) {
    let mut min: Option<i64> = None;
    let mut max: Option<i64> = None;
    for part in args.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("min:")
            && let Ok(v) = rest.trim().parse()
        {
            min = Some(v);
        } else if let Some(rest) = part.strip_prefix("max:")
            && let Ok(v) = rest.trim().parse()
        {
            max = Some(v);
        }
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> cratestack_core::Schema {
        cratestack_parser::parse_schema(text).expect("schema parses")
    }

    fn payload(items: &[(&str, serde_json::Value)]) -> serde_json::Map<String, serde_json::Value> {
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn required_field_missing_on_create_errors() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                  body String?
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let errors = validate_payload(model, &payload(&[("id", serde_json::json!("a"))]), false);
        let codes: Vec<ValidationCode> = errors.iter().map(|e| e.code).collect();
        assert!(codes.contains(&ValidationCode::Required));
        // The optional body field must not error.
        assert!(!errors.iter().any(|e| e.field == "body"));
    }

    #[test]
    fn required_field_missing_on_update_is_ok() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let errors = validate_payload(model, &payload(&[]), true);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn email_validator_passes_and_fails() {
        let schema = parse(
            r#"
                model User {
                  id String @id
                  email String @email
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "User").unwrap();
        let ok = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("u1")),
                ("email", serde_json::json!("alice@example.com")),
            ]),
            false,
        );
        assert!(ok.is_empty(), "{ok:?}");

        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("u1")),
                ("email", serde_json::json!("not-an-email")),
            ]),
            false,
        );
        assert!(bad.iter().any(|e| e.code == ValidationCode::Email));
    }

    #[test]
    fn length_validator_enforces_min_and_max() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String @length(min: 3, max: 10)
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let short = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("p")),
                ("title", serde_json::json!("hi")),
            ]),
            false,
        );
        assert!(short.iter().any(|e| e.code == ValidationCode::Length));

        let long = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("p")),
                ("title", serde_json::json!("this is too long")),
            ]),
            false,
        );
        assert!(long.iter().any(|e| e.code == ValidationCode::Length));
    }

    #[test]
    fn range_validator_enforces_bounds() {
        let schema = parse(
            r#"
                model Customer {
                  id Int @id
                  age Int @range(min: 0, max: 120)
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Customer").unwrap();
        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!(1)),
                ("age", serde_json::json!(-1)),
            ]),
            false,
        );
        assert!(bad.iter().any(|e| e.code == ValidationCode::Range));
    }

    #[test]
    fn regex_validator_enforces_pattern() {
        let schema = parse(
            r#"
                model Code {
                  id String @id
                  slug String @regex("^[a-z][a-z0-9-]*$")
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Code").unwrap();
        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("c1")),
                ("slug", serde_json::json!("Bad Slug!")),
            ]),
            false,
        );
        assert!(bad.iter().any(|e| e.code == ValidationCode::Regex));
    }

    #[test]
    fn iso4217_requires_three_uppercase_letters() {
        let schema = parse(
            r#"
                model Account {
                  id String @id
                  currency String @iso4217
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Account").unwrap();
        let ok = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("a1")),
                ("currency", serde_json::json!("USD")),
            ]),
            false,
        );
        assert!(ok.is_empty(), "{ok:?}");

        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("a1")),
                ("currency", serde_json::json!("usd")),
            ]),
            false,
        );
        assert!(bad.iter().any(|e| e.code == ValidationCode::Iso4217));
    }

    #[test]
    fn type_mismatch_flags_wrong_json_type() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("p")),
                ("title", serde_json::json!(42)),
            ]),
            false,
        );
        assert!(bad.iter().any(|e| e.code == ValidationCode::TypeMismatch));
    }

    #[test]
    fn null_on_required_field_errors_but_null_on_optional_ok() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                  body String?
                }
            "#,
        );
        let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let bad = validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("p")),
                ("title", serde_json::json!(null)),
                ("body", serde_json::json!(null)),
            ]),
            false,
        );
        let title_err = bad.iter().any(|e| e.field == "title" && e.code == ValidationCode::Required);
        let body_err = bad.iter().any(|e| e.field == "body");
        assert!(title_err);
        assert!(!body_err);
    }
}
