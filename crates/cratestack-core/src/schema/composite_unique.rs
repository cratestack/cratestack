//! Parsing for the model-level `@@unique([...], where: "...")` composite
//! unique attribute (Prisma's spelling for the field list, extended with
//! a partial-index predicate by cratestack#742). Shares its bracketed
//! field-list syntax with `@@id([...])` (see [`super::field_list`]); what
//! differs from that plain form is the semantics — a composite unique
//! constraint, not the primary key — the error text pointing at the
//! single-field alternative, and the optional trailing `where:` keyword
//! argument, whose split/parse shape is shared with `@@index([...],
//! where: "...")` (cratestack#156) via [`super::attribute_syntax`].

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

#[cfg(test)]
mod tests {
    use super::parse_composite_unique_attribute;

    #[test]
    fn parses_two_fields() {
        let parsed = parse_composite_unique_attribute("@@unique([tenantId, name])").unwrap();
        assert_eq!(
            parsed.fields,
            vec!["tenantId".to_string(), "name".to_string()]
        );
        assert_eq!(parsed.where_predicate, None);
    }

    #[test]
    fn parses_three_fields_in_declared_order() {
        let parsed =
            parse_composite_unique_attribute("@@unique([tenantId, name, environment])").unwrap();
        assert_eq!(
            parsed.fields,
            vec![
                "tenantId".to_string(),
                "name".to_string(),
                "environment".to_string(),
            ]
        );
    }

    #[test]
    fn parses_where_predicate_on_a_composite_field_list() {
        let parsed = parse_composite_unique_attribute(
            "@@unique([tenantId, idempotencyKey], where: \"idempotency_key IS NOT NULL\")",
        )
        .unwrap();
        assert_eq!(
            parsed.fields,
            vec!["tenantId".to_string(), "idempotencyKey".to_string()]
        );
        assert_eq!(
            parsed.where_predicate,
            Some("idempotency_key IS NOT NULL".to_string())
        );
    }

    /// The motivating case (cratestack#742, ADR 0038's deferred B3): a
    /// single, genuinely-optional column that must be unique only when
    /// present. Field-level `@unique` can't express `where:` at all, so
    /// a single-field `@@unique([...], where: "...")` is the only way
    /// to spell this — unlike a *non*-partial single-field `@@unique`,
    /// which is still rejected (see `rejects_single_field` below) in
    /// favor of the simpler field-level form.
    #[test]
    fn parses_where_predicate_on_a_single_field_list() {
        let parsed = parse_composite_unique_attribute(
            "@@unique([idempotencyKey], where: \"idempotency_key IS NOT NULL\")",
        )
        .unwrap();
        assert_eq!(parsed.fields, vec!["idempotencyKey".to_string()]);
        assert_eq!(
            parsed.where_predicate,
            Some("idempotency_key IS NOT NULL".to_string())
        );
    }

    #[test]
    fn rejects_missing_brackets() {
        let error = parse_composite_unique_attribute("@@unique(tenantId, name)").unwrap_err();
        assert!(error.contains("must list fields as"), "error: {error}");
        assert!(
            error.contains("composite unique attribute"),
            "error: {error}"
        );
    }

    #[test]
    fn rejects_single_field() {
        let error = parse_composite_unique_attribute("@@unique([email])").unwrap_err();
        assert!(error.contains("at least two fields"), "error: {error}");
        assert!(error.contains("field-level `@unique`"), "error: {error}");
    }

    #[test]
    fn rejects_duplicate_field() {
        let error = parse_composite_unique_attribute("@@unique([name, name])").unwrap_err();
        assert!(error.contains("more than once"), "error: {error}");
    }

    #[test]
    fn rejects_invalid_identifier() {
        let error = parse_composite_unique_attribute("@@unique([tenant-id, name])").unwrap_err();
        assert!(error.contains("invalid field name"), "error: {error}");
    }

    #[test]
    fn rejects_other_attributes() {
        let error = parse_composite_unique_attribute("@@id([a, b])").unwrap_err();
        assert!(error.contains("unsupported composite unique attribute"));
    }

    #[test]
    fn rejects_unquoted_where_value() {
        let error = parse_composite_unique_attribute(
            "@@unique([tenantId, name], where: idempotency_key IS NOT NULL)",
        )
        .unwrap_err();
        assert!(
            error.contains("expected a quoted SQL predicate"),
            "error: {error}"
        );
    }

    #[test]
    fn rejects_empty_where_value() {
        let error = parse_composite_unique_attribute("@@unique([tenantId, name], where: \"\")")
            .unwrap_err();
        assert!(error.contains("empty `where` predicate"), "error: {error}");
    }

    #[test]
    fn rejects_duplicate_where_key() {
        let error = parse_composite_unique_attribute(
            "@@unique([tenantId, name], where: \"a\", where: \"b\")",
        )
        .unwrap_err();
        assert!(error.contains("more than once"), "error: {error}");
    }

    #[test]
    fn rejects_unsupported_key() {
        let error = parse_composite_unique_attribute("@@unique([tenantId, name], sorted: true)")
            .unwrap_err();
        assert!(error.contains("unsupported key"), "error: {error}");
    }
}
