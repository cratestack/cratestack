//! `?computedParams=` query-value parsing — split out of
//! [`super`] to stay under the repo's ~200-LoC file convention.

use std::collections::BTreeMap;

use cratestack_core::CratestackError;

/// Decodes the raw `?computedParams=` query value (already percent-decoded
/// by [`super::parse_query_pairs`]) into a `{ fieldName: paramsJson }` map.
/// Shared, schema-independent JSON-shape validation — "is this even a JSON
/// object" — lives here once rather than being re-emitted per model by the
/// macro; per-model concerns (which keys are legal for *this* model, does
/// the referenced field have a params type, is it excluded by `?fields=`)
/// stay in generated code, which is the only place that field list is
/// known. See `docs/design/computed-fields.md`'s "Parameterized resolvers
/// on the wire" section.
pub fn parse_computed_params_object(
    raw: &str,
) -> Result<BTreeMap<String, serde_json::Value>, CratestackError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        CratestackError::Validation(format!("computedParams must be valid JSON: {error}"))
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(CratestackError::Validation(
            "computedParams must be a JSON object".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_computed_params_object() {
        let map = parse_computed_params_object(r#"{"proxyUrl":{"width":800}}"#)
            .expect("object should parse");
        assert_eq!(
            map.get("proxyUrl"),
            Some(&serde_json::json!({ "width": 800 }))
        );
    }

    #[test]
    fn rejects_malformed_json() {
        let error = parse_computed_params_object("{not json").unwrap_err();
        assert!(matches!(error, CratestackError::Validation(_)));
    }

    #[test]
    fn rejects_non_object_json() {
        let error = parse_computed_params_object("[1,2,3]").unwrap_err();
        assert!(matches!(error, CratestackError::Validation(_)));
    }

    #[test]
    fn accepts_an_empty_object() {
        let map = parse_computed_params_object("{}").expect("empty object should parse");
        assert!(map.is_empty());
    }
}
