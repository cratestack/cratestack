//! Payload → `rusqlite::types::Value` bindings.
//!
//! Each incoming JSON payload key is matched against the model's
//! column list (in declaration order); present keys become bound
//! values, absent keys are skipped — the UPDATE path relies on that
//! for partial writes.

use crate::data::Row;
use crate::data::model_info::ModelSqlInfo;

/// Map the payload object to `(columns, bound_values)`. Both vectors
/// share an index, so the i-th column gets the i-th bind.
///
/// `version_column` (SQL name) is always skipped — see
/// `crate::data::postgres::bindings::collect_payload`'s doc comment for
/// why (same rationale, both dialects).
pub(super) fn build_payload_bindings(
    info: &ModelSqlInfo<'_>,
    payload: &Row,
    version_column: Option<&str>,
) -> (Vec<String>, Vec<rusqlite::types::Value>) {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for col in &info.columns {
        if Some(col.column_name.as_str()) == version_column {
            continue;
        }
        let Some(json_value) = payload.get(col.field_name) else {
            continue;
        };
        columns.push(col.column_name.clone());
        values.push(match col.scalar {
            "BigInt" => bigint_to_sqlite(json_value),
            _ => json_to_sqlite(json_value),
        });
    }
    (columns, values)
}

/// A `BigInt` field's payload value is a canonical decimal string on
/// every wire, and the embedded runtime reads the column back as an
/// `i64`, so it is stored as an SQLite INTEGER. [`json_to_sqlite`] would
/// store the string as TEXT, which a BLOB-affinity column accepts
/// silently and the embedded read then rejects. The payload was
/// validated upstream (`validators::check_type`); an unparsable string
/// stays TEXT here rather than being coerced into a number.
fn bigint_to_sqlite(value: &serde_json::Value) -> rusqlite::types::Value {
    match value
        .as_str()
        .and_then(|text| text.parse::<cratestack_core::BigInt>().ok())
    {
        Some(parsed) => rusqlite::types::Value::Integer(parsed.get()),
        None => json_to_sqlite(value),
    }
}

fn json_to_sqlite(value: &serde_json::Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match value {
        serde_json::Value::Null => V::Null,
        serde_json::Value::Bool(b) => V::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                V::Integer(i)
            } else if let Some(f) = n.as_f64() {
                V::Real(f)
            } else {
                V::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => V::Text(s.clone()),
        // JSON objects/arrays land as text — the schema-declared type
        // is the contract; we round-trip via SQLite's text storage
        // which lines up with how the framework's macro path stores
        // JSON columns.
        other => V::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::types::Value as V;

    use super::*;

    #[test]
    fn bigint_string_binds_as_an_sqlite_integer_not_text() {
        for (text, expected) in [
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
            ("9007199254740993", 9_007_199_254_740_993),
            ("0", 0),
        ] {
            assert_eq!(
                bigint_to_sqlite(&serde_json::json!(text)),
                V::Integer(expected),
                "{text}"
            );
        }
    }

    #[test]
    fn bigint_null_binds_null() {
        assert_eq!(bigint_to_sqlite(&serde_json::Value::Null), V::Null);
    }

    #[test]
    fn non_canonical_bigint_is_not_coerced_into_a_number() {
        for bad in ["+5", "007", "-0", " 1", "9223372036854775808"] {
            assert_eq!(
                bigint_to_sqlite(&serde_json::json!(bad)),
                V::Text(bad.to_owned()),
                "{bad}"
            );
        }
    }
}
