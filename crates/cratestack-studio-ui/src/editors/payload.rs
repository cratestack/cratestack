//! Build a JSON payload from a `String`-typed form map, dispatching
//! each value through the field's declared scalar type so the
//! validator gets a typed value rather than a string that happens to
//! look like a number.

use crate::types::FieldSummary;

/// `""` is the editor's universal "no value" sentinel — every widget
/// in `editors::render` (text, number, boolean/enum `<select>`) uses
/// it uniformly, and `app::fields::snapshot_for_edit` seeds it for a
/// `null` cell so this branch turns it back into `Value::Null`.
///
/// This means an *optional String* field can't distinguish "explicit
/// empty string" from "null" through this form: both round-trip as
/// `""` and both save as `Value::Null`. That is a real but narrow gap
/// — it only affects optional `String` columns, since every other
/// scalar type has no valid non-null value that formats to `""` — and
/// it predates and is independent of this fix (it already existed via
/// the Boolean/Enum `<select>`'s own `""` "unset" option). Resolving
/// it would need a tri-state per optional field (e.g. a "set to null"
/// toggle next to the input) across `editors::render`, this module,
/// and `app::fields` — out of scope here; tracked as a follow-up
/// rather than bolted on to a NULL-corruption fix.
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
