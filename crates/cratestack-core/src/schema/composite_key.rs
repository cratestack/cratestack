//! Parsing for the model-level `@@id([...])` composite-primary-key
//! attribute (Prisma's spelling). Mirrors [`crate::events::parse_emit_attribute`]'s
//! shape: syntax parsing lives here in `cratestack-core` so both the
//! parser's semantic checker and any other consumer share one
//! implementation.

/// Parses `@@id([field1, field2, ...])` into its ordered list of local
/// field names. Callers are responsible for checking that each name
/// resolves to a real scalar field on the model.
pub fn parse_composite_id_attribute(raw: &str) -> Result<Vec<String>, String> {
    let Some(inner) = raw
        .strip_prefix("@@id(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(format!("unsupported composite id attribute `{raw}`"));
    };

    let Some(list) = inner
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err(format!(
            "composite id attribute `{raw}` must list fields as `@@id([field1, field2])`"
        ));
    };

    let mut fields = Vec::new();
    for part in list.split(',').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        if !is_valid_field_name(part) {
            return Err(format!(
                "composite id attribute `{raw}` lists invalid field name `{part}`"
            ));
        }
        if fields.contains(&part.to_owned()) {
            return Err(format!(
                "composite id attribute `{raw}` lists field `{part}` more than once"
            ));
        }
        fields.push(part.to_owned());
    }

    if fields.len() < 2 {
        return Err(format!(
            "composite id attribute `{raw}` must list at least two fields; use a single-field `@id` instead"
        ));
    }

    Ok(fields)
}

fn is_valid_field_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
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
