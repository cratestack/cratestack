//! Parsing for the model-level `@@index([...], using: ..., opclass: "...",
//! where: "...")` attribute (cratestack#156, cratestack#742) — a general
//! secondary index, optionally naming a non-default Postgres access
//! method (e.g. `ivfflat`/`hnsw` for pgvector approximate-nearest-neighbor
//! search), operator class, and a partial-index predicate.
//!
//! Shares its bracketed-field-list syntax with `@@id([...])`/
//! `@@unique([...])` (see [`super::field_list`]), but unlike those,
//! `@@index(...)` also accepts trailing `using:`/`opclass:`/`where:`
//! keyword arguments after the field list — the same shape
//! `@relation(fields:[...], references:[...])` already uses
//! (`cratestack-parser::relation_helpers::parse_relation_attribute`),
//! reimplemented here (via [`super::attribute_syntax`], shared with
//! `@@unique([...], where: "...")`) rather than shared with the parser
//! crate's own copy because that helper is private to the parser crate
//! and this one also needs to be callable from `cratestack-migrate` (via
//! `crate::schema::project_model`, one layer below the parser).

#[cfg(test)]
mod tests;

use super::attribute_syntax::{
    parse_bracketed_field_list, parse_where_value, split_top_level_commas,
};

const LABEL: &str = "index attribute";
const KEYWORD: &str = "@@index";

/// The parsed shape of an `@@index([...], using: ..., opclass: "...",
/// where: "...")` attribute.
///
/// `using`/`opclass`/`where` are carried through verbatim — not
/// validated against a closed list of Postgres access methods/operator
/// classes/predicate grammar. Per `docs/design/extensions.md` §2/§6, the
/// framework deliberately avoids hardcoding pgvector's own index types
/// as the only supported similarity-search backend, so any syntactically
/// valid identifier (or, for `where`, any quoted text) is accepted here
/// and left for Postgres itself to accept or reject at `CREATE INDEX`
/// time.
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
    /// `where: "<sql predicate>"` (cratestack#742) — a partial-index
    /// condition, carried verbatim into a trailing `WHERE <predicate>`
    /// on the emitted `CREATE INDEX`. `None` means the index covers
    /// every row, which renders byte-identical DDL to before this field
    /// existed.
    pub where_predicate: Option<String>,
}

/// Parses `@@index([field1, field2, ...])`, optionally followed by
/// `using: <method>`, `opclass: "<name>"`, and/or `where: "<predicate>"`.
/// Callers are responsible for checking that each field name resolves to
/// a real scalar field on the model.
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
    let fields = parse_bracketed_field_list(raw, entries.remove(0), LABEL, KEYWORD)?;
    if fields.is_empty() {
        return Err(format!(
            "index attribute `{raw}` must list at least one field"
        ));
    }

    let mut using = None;
    let mut opclass = None;
    let mut where_predicate = None;
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
            "where" if where_predicate.is_none() => {
                where_predicate = Some(parse_where_value(raw, value, LABEL)?);
            }
            "where" => {
                return Err(format!(
                    "index attribute `{raw}` declares `where` more than once"
                ));
            }
            other => {
                return Err(format!(
                    "index attribute `{raw}` has unsupported key `{other}`; expected `using`, \
                     `opclass`, or `where`"
                ));
            }
        }
    }

    Ok(ParsedIndexAttribute {
        fields,
        using,
        opclass,
        where_predicate,
    })
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
