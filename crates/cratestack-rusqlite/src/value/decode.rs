//! Free decode helpers for the TEXT-stored rich types.

use cratestack_core::Value;
use rusqlite::types::{FromSqlError, FromSqlResult};

/// Decode a typed value from a column. Use the marker types below to pick a
/// concrete representation — `Uuid`, `DateTime<Utc>`, `Value`, `Decimal` all
/// need explicit choices since SQLite stores them as TEXT.
pub fn decode_uuid(raw: &str) -> FromSqlResult<uuid::Uuid> {
    uuid::Uuid::parse_str(raw).map_err(|error| FromSqlError::Other(Box::new(error)))
}

pub fn decode_datetime(raw: &str) -> FromSqlResult<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|error| FromSqlError::Other(Box::new(error)))
}

pub fn decode_json(raw: &str) -> FromSqlResult<Value> {
    serde_json::from_str(raw).map_err(|error| FromSqlError::Other(Box::new(error)))
}

pub fn decode_decimal(raw: &str) -> FromSqlResult<cratestack_core::Decimal> {
    raw.parse::<cratestack_core::Decimal>().map_err(|error| {
        FromSqlError::Other(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid decimal value: {error}"),
        )))
    })
}
