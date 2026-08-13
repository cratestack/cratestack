//! Parsing for the model-level `@@index([...], using: ..., opclass: "...")`
//! attribute (cratestack#156) — a general secondary index, optionally
//! naming a non-default Postgres access method (e.g. `ivfflat`/`hnsw` for
//! pgvector approximate-nearest-neighbor search) and operator class.
//!
//! Shares its bracketed-field-list syntax with `@@id([...])`/
//! `@@unique([...])` (see [`super::field_list`]), but unlike those,
//! `@@index(...)` also accepts trailing `using:`/`opclass:` keyword
//! arguments after the field list — the same shape
//! `@relation(fields:[...], references:[...])` already uses
//! (`cratestack-parser::relation_helpers::parse_relation_attribute`),
//! reimplemented here rather than shared because that helper is private to
//! the parser crate and this one also needs to be callable from
//! `cratestack-migrate` (via `crate::schema::project_model`, one layer
//! below the parser).

#[cfg(test)]
mod tests;

use super::field_list::is_valid_field_name;

/// The parsed shape of an `@@index([...], using: ..., opclass: "...")`
/// attribute.
///
/// `using`/`opclass` are carried through verbatim — not validated against
/// a closed list of Postgres access methods/operator classes. Per
/// `docs/design/extensions.md` §2/§6, the framework deliberately avoids
/// hardcoding pgvector's own index types as the only supported
/// similarity-search backend, so any syntactically valid identifier is
/// accepted here and left for Postgres itself to accept or reject at
/// `CREATE INDEX` time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIndexAttribute {
    /// Local field names, in declaration order — the index's column
    /// order.
    pub fields: Vec<String>,
    /// `using: <method>` — e.g. `ivfflat`, `hnsw`, `gin`. `None` means no
    /// access method was named, which renders as a plain `CREATE INDEX`
    /// with Postgres's own default (`btree`) left implicit.
    pub using: Option<String>,
    /// `opclass: "<name>"` — applied to every column listed in `fields`.
    /// `None` leaves each column's default operator class in place.
    pub opclass: Option<String>,
}

/// Parses `@@index([field1, field2, ...])`, optionally followed by
/// `using: <method>` and/or `opclass: "<name>"`. Callers are responsible
/// for checking that each field name resolves to a real scalar field on
/// the model.
pub fn parse_index_attribute(raw: &str) -> Result<ParsedIndexAttribute, String> {
    let inner = raw
        .strip_prefix("@@index(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| format!("unsupported index attribute `{raw}`"))?;

    let mut entries = split_top_level_commas(inner);
    if entries.is_empty() {
        return Err(format!(
            "index attribute `{raw}` must list fields as `@@index([field1, field2])`"
        ));
    }
    let fields = parse_bracketed_field_list(raw, entries.remove(0))?;
    if fields.is_empty() {
        return Err(format!(
            "index attribute `{raw}` must list at least one field"
        ));
    }

    let mut using = None;
    let mut opclass = None;
    for entry in entries {
        let (key, value) = entry
            .split_once(':')
            .ok_or_else(|| format!("index attribute `{raw}` has invalid entry `{entry}`"))?;
        let value = value.trim();
        match key.trim() {
            "using" if using.is_none() => using = Some(parse_using_value(raw, value)?),
            "using" => {
                return Err(format!(
                    "index attribute `{raw}` declares `using` more than once"
                ));
            }
            "opclass" if opclass.is_none() => opclass = Some(parse_opclass_value(raw, value)?),
            "opclass" => {
                return Err(format!(
                    "index attribute `{raw}` declares `opclass` more than once"
                ));
            }
            other => {
                return Err(format!(
                    "index attribute `{raw}` has unsupported key `{other}`; expected `using` or `opclass`"
                ));
            }
        }
    }

    Ok(ParsedIndexAttribute {
        fields,
        using,
        opclass,
    })
}

fn parse_bracketed_field_list(raw: &str, entry: &str) -> Result<Vec<String>, String> {
    let list = entry
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| {
            format!("index attribute `{raw}` must list fields as `@@index([field1, field2])`")
        })?;

    let mut fields = Vec::new();
    for part in list.split(',').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        if !is_valid_field_name(part) {
            return Err(format!(
                "index attribute `{raw}` lists invalid field name `{part}`"
            ));
        }
        if fields.contains(&part.to_owned()) {
            return Err(format!(
                "index attribute `{raw}` lists field `{part}` more than once"
            ));
        }
        fields.push(part.to_owned());
    }
    Ok(fields)
}

fn parse_using_value(raw: &str, value: &str) -> Result<String, String> {
    if !is_valid_identifier(value) {
        return Err(format!(
            "index attribute `{raw}` has invalid `using` value `{value}`; expected a bare \
             access method name like `ivfflat`"
        ));
    }
    Ok(value.to_owned())
}

fn parse_opclass_value(raw: &str, value: &str) -> Result<String, String> {
    let inner = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| {
            format!(
                "index attribute `{raw}` has invalid `opclass` value `{value}`; expected a \
                 quoted string like `\"vector_l2_ops\"`"
            )
        })?;
    if !is_valid_identifier(inner) {
        return Err(format!(
            "index attribute `{raw}` has invalid `opclass` value `\"{inner}\"`"
        ));
    }
    Ok(inner.to_owned())
}

fn is_valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Splits `input` on top-level commas — ones not nested inside `[...]` —
/// so the bracketed field list's own internal commas aren't mistaken for
/// separators between it and the trailing `using:`/`opclass:` entries.
/// Mirrors `cratestack-parser::relation_helpers::split_top_level`
/// (private to that crate; small enough to not be worth sharing across
/// the crate boundary).
fn split_top_level_commas(input: &str) -> Vec<&str> {
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
