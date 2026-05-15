//! Attribute-level predicates mirroring the macro path's generated
//! `validate` impls. Each predicate returns `Some(FieldError)` when
//! the value fails the rule, `None` when it passes (or when the value
//! shape doesn't match — e.g. `@email` on a non-string is silently
//! skipped here because [`super::check_type`] will have already
//! flagged the type mismatch).

use cratestack_core::Field;

use super::types::{FieldError, ValidationCode};

/// Dispatch one attribute against a value. Returns `None` for any
/// attribute Studio doesn't validate (the schema can carry arbitrary
/// macro-only attributes that we don't mirror).
pub(super) fn run_attribute(
    field: &Field,
    raw: &str,
    value: &serde_json::Value,
) -> Option<FieldError> {
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
    let bad = s.matches('@').count() != 1
        || !s.contains('.')
        || s.starts_with('@')
        || s.ends_with('@')
        || s.contains(' ');
    bad.then(|| FieldError {
        field: field.name.clone(),
        code: ValidationCode::Email,
        message: format!("field '{}' is not a valid email address", field.name),
    })
}

fn check_uri(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let s = value.as_str()?;
    url::Url::parse(s).err().map(|_| FieldError {
        field: field.name.clone(),
        code: ValidationCode::Uri,
        message: format!("field '{}' is not a valid URI", field.name),
    })
}

fn check_iso4217(field: &Field, value: &serde_json::Value) -> Option<FieldError> {
    let s = value.as_str()?;
    let bad = s.len() != 3 || !s.chars().all(|c| c.is_ascii_uppercase());
    bad.then(|| FieldError {
        field: field.name.clone(),
        code: ValidationCode::Iso4217,
        message: format!(
            "field '{}' must be a 3-letter uppercase ISO 4217 currency code",
            field.name
        ),
    })
}

fn check_length(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let s = value.as_str()?;
    let len = s.chars().count() as i64;
    let (min, max) = parse_min_max(args);
    if let Some(m) = min
        && len < m
    {
        return Some(length_error(field, m, "at least"));
    }
    if let Some(m) = max
        && len > m
    {
        return Some(length_error(field, m, "at most"));
    }
    None
}

fn length_error(field: &Field, bound: i64, comparator: &str) -> FieldError {
    FieldError {
        field: field.name.clone(),
        code: ValidationCode::Length,
        message: format!(
            "field '{}' must be {comparator} {bound} character{} long",
            field.name,
            if bound == 1 { "" } else { "s" }
        ),
    }
}

fn check_range(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let n = value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))?;
    let (min, max) = parse_min_max(args);
    if let Some(m) = min
        && n < m
    {
        return Some(range_error(field, m, "at least"));
    }
    if let Some(m) = max
        && n > m
    {
        return Some(range_error(field, m, "at most"));
    }
    None
}

fn range_error(field: &Field, bound: i64, comparator: &str) -> FieldError {
    FieldError {
        field: field.name.clone(),
        code: ValidationCode::Range,
        message: format!("field '{}' must be {comparator} {bound}", field.name),
    }
}

fn check_regex(field: &Field, value: &serde_json::Value, args: &str) -> Option<FieldError> {
    let s = value.as_str()?;
    let pattern = args.trim().trim_matches('"');
    let regex = regex::Regex::new(pattern).ok()?;
    (!regex.is_match(s)).then(|| FieldError {
        field: field.name.clone(),
        code: ValidationCode::Regex,
        message: format!("field '{}' did not match the required pattern", field.name),
    })
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
