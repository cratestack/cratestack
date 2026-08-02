//! Parsing for the model-level `@@unique([...])` composite-unique
//! attribute (Prisma's spelling). Shares its syntax rules with
//! `@@id([...])` via [`super::field_list`]; what differs is the
//! semantics — a composite unique constraint, not the primary key —
//! and the error text pointing at the single-field alternative.

use super::field_list::{FieldListSpec, parse_field_list};

const SPEC: FieldListSpec = FieldListSpec {
    prefix: "@@unique(",
    label: "composite unique attribute",
    example: "@@unique([field1, field2])",
};

/// Parses `@@unique([field1, field2, ...])` into its ordered list of
/// local field names. Callers are responsible for checking that each
/// name resolves to a real scalar field on the model.
///
/// The order is significant: it is the column order of the emitted
/// unique index, and it feeds the index name
/// (`<table>_<col1>_<col2>_key`), so reordering the list is a rename.
pub fn parse_composite_unique_attribute(raw: &str) -> Result<Vec<String>, String> {
    let fields = parse_field_list(raw, &SPEC)?;
    if fields.len() < 2 {
        return Err(format!(
            "composite unique attribute `{raw}` must list at least two fields; use a field-level `@unique` instead"
        ));
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::parse_composite_unique_attribute;

    #[test]
    fn parses_two_fields() {
        let fields = parse_composite_unique_attribute("@@unique([tenantId, name])").unwrap();
        assert_eq!(fields, vec!["tenantId".to_string(), "name".to_string()]);
    }

    #[test]
    fn parses_three_fields_in_declared_order() {
        let fields =
            parse_composite_unique_attribute("@@unique([tenantId, name, environment])").unwrap();
        assert_eq!(
            fields,
            vec![
                "tenantId".to_string(),
                "name".to_string(),
                "environment".to_string(),
            ]
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
}
