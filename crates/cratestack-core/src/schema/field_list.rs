//! Shared parsing for model-level attributes whose sole argument is a
//! bracketed list of local field names — `@@id([...])` (see
//! [`super::composite_key`]) and `@@unique([...])` (see
//! [`super::composite_unique`]).
//!
//! The two attributes differ only in their keyword and in what a caller
//! does with the resulting field names, so the syntax rules — brackets
//! required, identifiers well-formed, no repeats — live here once.

/// Per-attribute strings woven into the syntax errors, so a malformed
/// `@@unique` never reports itself as a malformed `@@id`.
pub(crate) struct FieldListSpec {
    /// Attribute keyword including its opening paren, e.g. `"@@id("`.
    pub(crate) prefix: &'static str,
    /// Human-readable name used in errors, e.g. `"composite id attribute"`.
    pub(crate) label: &'static str,
    /// Well-formed example shown when the brackets are missing.
    pub(crate) example: &'static str,
}

/// Parses `<keyword>([field1, field2, ...])` into its ordered list of
/// local field names. No minimum length is enforced here — that rule
/// differs per attribute and belongs to the caller.
pub(crate) fn parse_field_list(raw: &str, spec: &FieldListSpec) -> Result<Vec<String>, String> {
    let label = spec.label;
    let Some(inner) = raw
        .strip_prefix(spec.prefix)
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(format!("unsupported {label} `{raw}`"));
    };

    let Some(list) = inner
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err(format!(
            "{label} `{raw}` must list fields as `{}`",
            spec.example
        ));
    };

    let mut fields = Vec::new();
    for part in list.split(',').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        if !is_valid_field_name(part) {
            return Err(format!("{label} `{raw}` lists invalid field name `{part}`"));
        }
        if fields.contains(&part.to_owned()) {
            return Err(format!(
                "{label} `{raw}` lists field `{part}` more than once"
            ));
        }
        fields.push(part.to_owned());
    }

    Ok(fields)
}

/// Visible to sibling schema submodules (`pub(super)` = `crate::schema`)
/// so [`super::index_attribute`] can validate its own field list with the
/// exact same identifier rule, without duplicating it.
pub(super) fn is_valid_field_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
