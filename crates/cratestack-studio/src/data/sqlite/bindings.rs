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
pub(super) fn build_payload_bindings(
    info: &ModelSqlInfo<'_>,
    payload: &Row,
) -> (Vec<String>, Vec<rusqlite::types::Value>) {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for col in &info.columns {
        let Some(json_value) = payload.get(col.field_name) else {
            continue;
        };
        columns.push(col.column_name.clone());
        values.push(json_to_sqlite(json_value));
    }
    (columns, values)
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
