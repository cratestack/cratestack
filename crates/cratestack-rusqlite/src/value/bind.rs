//! `SqlValue` → rusqlite binding adapter.
//!
//! TEXT-as-canonical storage for Uuid / DateTime / Json / Decimal. See the
//! parent module docs for the storage-class map.

use cratestack_core::Value;
use cratestack_sql::SqlValue;
use rusqlite::ToSql;
use rusqlite::types::{ToSqlOutput, ValueRef};

/// Adapter that lets a borrowed `SqlValue` be bound by rusqlite.
pub struct SqlValueParam<'a>(pub &'a SqlValue);

impl<'a> ToSql for SqlValueParam<'a> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        use rusqlite::types::Value as RV;

        let value: ToSqlOutput<'_> = match self.0 {
            SqlValue::Bool(v) => ToSqlOutput::Owned(RV::Integer(i64::from(*v))),
            SqlValue::Int(v) => ToSqlOutput::Owned(RV::Integer(*v)),
            SqlValue::Float(v) => ToSqlOutput::Owned(RV::Real(*v)),
            SqlValue::String(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
            SqlValue::Bytes(v) => ToSqlOutput::Borrowed(ValueRef::Blob(v.as_slice())),
            SqlValue::Uuid(v) => ToSqlOutput::Owned(RV::Text(v.hyphenated().to_string())),
            SqlValue::DateTime(v) => ToSqlOutput::Owned(RV::Text(format_datetime(v))),
            SqlValue::Json(v) => ToSqlOutput::Owned(RV::Text(format_json(v))),
            SqlValue::Decimal(v) => ToSqlOutput::Owned(RV::Text(format_decimal(v))),
            SqlValue::NullBool
            | SqlValue::NullInt
            | SqlValue::NullFloat
            | SqlValue::NullString
            | SqlValue::NullBytes
            | SqlValue::NullUuid
            | SqlValue::NullDateTime
            | SqlValue::NullJson
            | SqlValue::NullDecimal => ToSqlOutput::Owned(RV::Null),
        };
        Ok(value)
    }
}

fn format_datetime(value: &chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn format_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn format_decimal(value: &cratestack_core::Decimal) -> String {
    value.to_string()
}
