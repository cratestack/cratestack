//! Parse `@rename(from = "...")` / `@@rename(from = "...")` markers.

use cratestack_core::{Field, Model};

pub(super) fn model_rename_from(model: &Model) -> Option<String> {
    let raw = model
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@@rename("))?
        .raw
        .as_str();
    parse_rename_from(raw, "@@rename(")
}

pub(super) fn field_rename_from(field: &Field) -> Option<String> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@rename("))?
        .raw
        .as_str();
    parse_rename_from(raw, "@rename(")
}

/// Extract the `<old>` value from `@rename(from = "<old>")` or
/// `@@rename(from = "<old>")`. Returns `None` for malformed input —
/// the diff engine treats malformed renames as if the attribute were
/// absent, falling back to drop+add. A future slice can promote this
/// to a parse-time validation error.
fn parse_rename_from(raw: &str, prefix: &str) -> Option<String> {
    let inner = raw.strip_prefix(prefix)?.strip_suffix(')')?.trim();
    let rest = inner.strip_prefix("from")?.trim_start();
    let value_part = rest.strip_prefix('=')?.trim_start();
    let unquoted = value_part
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))?;
    Some(unquoted.to_owned())
}
