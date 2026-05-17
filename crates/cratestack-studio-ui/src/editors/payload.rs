//! Build a JSON payload from a `String`-typed form map, dispatching
//! each value through the field's declared scalar type so the
//! validator gets a typed value rather than a string that happens to
//! look like a number.

use crate::types::FieldSummary;

pub fn build_payload(
    writable: &[FieldSummary],
    values: &std::collections::BTreeMap<String, String>,
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for f in writable {
        let raw = values.get(&f.name).cloned().unwrap_or_default();
        if raw.is_empty() {
            if f.arity == "optional" {
                out.insert(f.name.clone(), serde_json::Value::Null);
            }
            continue;
        }
        out.insert(f.name.clone(), parse_value(f, &raw));
    }
    serde_json::Value::Object(out)
}

fn parse_value(f: &FieldSummary, raw: &str) -> serde_json::Value {
    if f.is_enum {
        return serde_json::Value::String(raw.to_owned());
    }
    match f.type_name.as_str() {
        "Int" => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_owned())),
        "Float" => raw
            .parse::<f64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_owned())),
        "Decimal" => serde_json::Value::String(raw.to_owned()),
        "DateTime" => serde_json::Value::String(normalize_datetime(raw)),
        "Boolean" => match raw {
            "true" | "1" | "yes" => serde_json::Value::Bool(true),
            "false" | "0" | "no" => serde_json::Value::Bool(false),
            other => serde_json::Value::String(other.to_owned()),
        },
        "Json" => serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_owned())),
        _ => serde_json::Value::String(raw.to_owned()),
    }
}

/// `<input type="datetime-local">` returns `YYYY-MM-DDTHH:MM` (and
/// `YYYY-MM-DDTHH:MM:SS` when `step` is set). Either way we tack a
/// `Z` on so the backend sees an unambiguous UTC instant rather than
/// a local-time string that callers later have to guess at.
pub(super) fn normalize_datetime(raw: &str) -> String {
    if raw.ends_with('Z') || raw.contains('+') {
        return raw.to_owned();
    }
    if raw.len() == 16 {
        return format!("{raw}:00Z");
    }
    format!("{raw}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_appends_zero_seconds_and_z() {
        assert_eq!(normalize_datetime("2024-02-03T04:05"), "2024-02-03T04:05:00Z");
        assert_eq!(normalize_datetime("2024-02-03T04:05:06"), "2024-02-03T04:05:06Z");
    }

    #[test]
    fn datetime_leaves_trailing_zone_alone() {
        assert_eq!(
            normalize_datetime("2024-02-03T04:05:06Z"),
            "2024-02-03T04:05:06Z"
        );
        assert_eq!(
            normalize_datetime("2024-02-03T04:05:06+01:00"),
            "2024-02-03T04:05:06+01:00"
        );
    }
}
