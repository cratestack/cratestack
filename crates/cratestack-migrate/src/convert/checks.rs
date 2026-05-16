//! Project `@db_enforce`-annotated validator attributes into the IR's
//! `CheckKind` set.

use cratestack_core::Field;

use crate::ir::CheckKind;

pub(super) fn field_has_db_enforce(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@db_enforce")
}

/// Collect every eligible validator attribute on `field` as a
/// [`CheckKind`]. Eligibility matches the ADR 0004 list: `@range`,
/// `@length`, `@iso4217`. Validators that don't translate cleanly to
/// SQL (`@email`, `@uri`, `@regex`) are skipped silently here — a
/// future parser-level validation slice can promote `@db_enforce` on
/// an ineligible validator to a parse-time error.
pub(super) fn collect_check_kinds(field: &Field) -> Vec<CheckKind> {
    let mut out = Vec::new();
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        if let Some(args) = strip_call(raw, "@range") {
            let (min, max) = parse_int_min_max(args);
            out.push(CheckKind::Range { min, max });
        } else if let Some(args) = strip_call(raw, "@length") {
            let (min, max) = parse_int_min_max(args);
            out.push(CheckKind::Length { min, max });
        } else if raw == "@iso4217" {
            out.push(CheckKind::Iso4217);
        }
    }
    out
}

pub(super) fn check_kind_slug(kind: &CheckKind) -> &'static str {
    match kind {
        CheckKind::Range { .. } => "range",
        CheckKind::Length { .. } => "length",
        CheckKind::Iso4217 => "iso4217",
    }
}

/// `@validator(...)` → `Some("...")`. Returns `None` when `raw` is
/// not a call to `validator`.
fn strip_call<'a>(raw: &'a str, validator: &str) -> Option<&'a str> {
    let after_name = raw.strip_prefix(validator)?;
    let inner = after_name.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner)
}

/// Parse `min: 0, max: 100` / `min: 0` / `max: 100` into `(min, max)`.
/// Tolerates whitespace and missing fields. Returns `(None, None)` on
/// any malformed input — the validator-level parser in
/// `cratestack-parser` has already rejected garbage by this point.
fn parse_int_min_max(args: &str) -> (Option<i64>, Option<i64>) {
    let mut min = None;
    let mut max = None;
    for part in args.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("min") {
            let value = rest.trim_start().strip_prefix(':').map(str::trim);
            if let Some(value) = value.and_then(|v| v.parse::<i64>().ok()) {
                min = Some(value);
            }
        } else if let Some(rest) = part.strip_prefix("max") {
            let value = rest.trim_start().strip_prefix(':').map(str::trim);
            if let Some(value) = value.and_then(|v| v.parse::<i64>().ok()) {
                max = Some(value);
            }
        }
    }
    (min, max)
}
