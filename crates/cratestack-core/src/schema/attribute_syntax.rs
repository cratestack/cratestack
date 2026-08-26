//! Shared parsing helpers for model-level attributes that combine a
//! bracketed field list with trailing `key: value` keyword arguments —
//! `@@index([...], using: ..., opclass: "...", where: "...")`
//! (cratestack#156, cratestack#742) and `@@unique([...], where: "...")`
//! (cratestack#742). `@@id([...])` never takes keyword arguments, so it
//! stays on the simpler [`super::field_list`] helpers instead of this
//! module.

use super::field_list::is_valid_field_name;

/// Splits `input` on top-level commas — ones not nested inside
/// `[...]`/`(...)` — so a bracketed field list's own internal commas, or
/// a parenthesized sub-expression inside a `where:` predicate, aren't
/// mistaken for the separator between the field list and the trailing
/// keyword arguments. A predicate containing a top-level, unparenthesized
/// comma (rare in a boolean expression, but not impossible) would still
/// split incorrectly — accepted as a known limitation of not running a
/// real SQL parser here, same posture as the rest of this module.
pub(super) fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                entries.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        entries.push(tail);
    }
    entries
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// Parses the first split entry (`[field1, field2]`) into its ordered
/// list of local field names. `label` (a human-readable name, e.g.
/// `"index attribute"`) and `keyword` (the attribute's own spelling,
/// e.g. `"@@index"`) feed the error text so a malformed `@@unique` field
/// list isn't reported as a malformed `@@index`, or vice versa.
pub(super) fn parse_bracketed_field_list(
    raw: &str,
    entry: &str,
    label: &str,
    keyword: &str,
) -> Result<Vec<String>, String> {
    let list = entry
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| {
            format!("{label} `{raw}` must list fields as `{keyword}([field1, field2])`")
        })?;

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

/// Parses a `where: "<sql predicate>"` keyword argument. Carried
/// through verbatim — see `docs/design/extensions.md` §2/§6 and the
/// `using`/`opclass` precedent this follows (cratestack#156): the
/// predicate text itself is never parsed or validated beyond unwrapping
/// its quotes, and is left for the database to accept or reject at
/// `CREATE INDEX` time.
pub(super) fn parse_where_value(raw: &str, value: &str, label: &str) -> Result<String, String> {
    let inner = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| {
            format!(
                "{label} `{raw}` has invalid `where` value `{value}`; expected a quoted SQL \
                 predicate like `\"idempotency_key IS NOT NULL\"`"
            )
        })?;
    if inner.trim().is_empty() {
        return Err(format!("{label} `{raw}` has an empty `where` predicate"));
    }
    Ok(inner.to_owned())
}
