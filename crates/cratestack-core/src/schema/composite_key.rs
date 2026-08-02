//! Parsing for the model-level `@@id([...])` composite-primary-key
//! attribute (Prisma's spelling). Mirrors [`crate::events::parse_emit_attribute`]'s
//! shape: syntax parsing lives here in `cratestack-core` so both the
//! parser's semantic checker and any other consumer share one
//! implementation. The bracketed-field-list syntax itself is shared
//! with `@@unique([...])` — see [`super::field_list`].

use super::field_list::{FieldListSpec, parse_field_list};

const SPEC: FieldListSpec = FieldListSpec {
    prefix: "@@id(",
    label: "composite id attribute",
    example: "@@id([field1, field2])",
};

/// Parses `@@id([field1, field2, ...])` into its ordered list of local
/// field names. Callers are responsible for checking that each name
/// resolves to a real scalar field on the model.
pub fn parse_composite_id_attribute(raw: &str) -> Result<Vec<String>, String> {
    let fields = parse_field_list(raw, &SPEC)?;
    if fields.len() < 2 {
        return Err(format!(
            "composite id attribute `{raw}` must list at least two fields; use a single-field `@id` instead"
        ));
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::parse_composite_id_attribute;

    #[test]
    fn parses_two_fields() {
        let fields = parse_composite_id_attribute("@@id([accountId, subject])").unwrap();
        assert_eq!(fields, vec!["accountId".to_string(), "subject".to_string()]);
    }

    #[test]
    fn rejects_missing_brackets() {
        let error = parse_composite_id_attribute("@@id(accountId, subject)").unwrap_err();
        assert!(error.contains("must list fields as"));
    }

    #[test]
    fn rejects_single_field() {
        let error = parse_composite_id_attribute("@@id([accountId])").unwrap_err();
        assert!(error.contains("at least two fields"));
    }

    #[test]
    fn rejects_duplicate_field() {
        let error = parse_composite_id_attribute("@@id([accountId, accountId])").unwrap_err();
        assert!(error.contains("more than once"));
    }

    #[test]
    fn rejects_invalid_identifier() {
        let error = parse_composite_id_attribute("@@id([account-id, subject])").unwrap_err();
        assert!(error.contains("invalid field name"));
    }
}
