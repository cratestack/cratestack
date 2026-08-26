//! Parsing for the model-level `@@unique([...], where: "...")` composite
//! unique attribute (Prisma's spelling for the field list, extended with
//! a partial-index predicate by cratestack#742). Its bracketed field-list
//! syntax is shared with `@@index([...])`/`@@id([...])` only via
//! [`super::field_list::is_valid_field_name`] (the identifier rule) — the
//! split/keyword-argument parsing itself is shared with `@@index([...],
//! where: "...")` (cratestack#156) through [`super::attribute_syntax`]
//! instead, since unlike `@@id([...])` this attribute also takes a
//! trailing `where:` keyword argument. What differs from `@@id([...])`'s
//! plain form is the semantics — a composite unique constraint, not the
//! primary key — and the error text pointing at the single-field
//! alternative.

#[cfg(test)]
mod tests;

use super::attribute_syntax::{
    parse_bracketed_field_list, parse_where_value, split_top_level_commas,
};

const LABEL: &str = "composite unique attribute";
const KEYWORD: &str = "@@unique";

/// The parsed shape of an `@@unique([...], where: "...")` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCompositeUnique {
    /// Local field names, in declaration order — the unique index's
    /// column order. It's also part of the generated index name
    /// (`<table>_<col1>_<col2>_key`), so reordering the list is a
    /// rename.
    pub fields: Vec<String>,
    /// `where: "<sql predicate>"` (cratestack#742) — makes the
    /// constraint a *partial* unique index (unique only among rows
    /// matching the predicate), carried verbatim into a trailing
    /// `WHERE <predicate>` on the emitted `CREATE UNIQUE INDEX`. `None`
    /// means the constraint applies to every row, which renders
    /// byte-identical DDL to before this field existed.
    pub where_predicate: Option<String>,
}

/// Parses `@@unique([field1, field2, ...], where: "...")` into its
/// ordered list of local field names plus an optional partial-index
/// predicate. Callers are responsible for checking that each name
/// resolves to a real scalar field on the model.
pub fn parse_composite_unique_attribute(raw: &str) -> Result<ParsedCompositeUnique, String> {
    let inner = raw
        .strip_prefix("@@unique(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| format!("unsupported {LABEL} `{raw}`"))?;

    let mut entries = split_top_level_commas(inner);
    if entries.is_empty() {
        return Err(format!(
            "{LABEL} `{raw}` must list fields as `{KEYWORD}([field1, field2])`"
        ));
    }
    let fields = parse_bracketed_field_list(raw, entries.remove(0), LABEL, KEYWORD)?;
    // Unconditional, matching `@@index`'s own check
    // (`index_attribute.rs`) — an empty field list is never valid, with
    // or without `where:`. The single-vs-two-field floor below is the
    // one rule `where:` relaxes; the zero floor isn't it.
    if fields.is_empty() {
        return Err(format!("{LABEL} `{raw}` must list at least one field"));
    }

    let mut where_predicate = None;
    for entry in entries {
        let (key, value) = entry
            .split_once(':')
            .ok_or_else(|| format!("{LABEL} `{raw}` has invalid entry `{entry}`"))?;
        match key.trim() {
            "where" if where_predicate.is_none() => {
                where_predicate = Some(parse_where_value(raw, value.trim(), LABEL)?);
            }
            "where" => {
                return Err(format!("{LABEL} `{raw}` declares `where` more than once"));
            }
            other => {
                return Err(format!(
                    "{LABEL} `{raw}` has unsupported key `{other}`; expected `where`"
                ));
            }
        }
    }

    // A single-field, non-partial `@@unique([x])` still has a strictly
    // simpler spelling (field-level `@unique`) — that alternative isn't
    // pointed at when `where:` is present, though, since field-level
    // `@unique` has no room for a keyword argument at all (issue #742's
    // motivating case, `@@unique([idempotencyKey], where: "...")`, is
    // exactly this: a genuinely single-column partial-unique index).
    if fields.len() < 2 && where_predicate.is_none() {
        return Err(format!(
            "{LABEL} `{raw}` must list at least two fields; use a field-level `@unique` instead"
        ));
    }

    Ok(ParsedCompositeUnique {
        fields,
        where_predicate,
    })
}
