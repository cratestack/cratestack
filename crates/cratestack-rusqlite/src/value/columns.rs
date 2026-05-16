//! Newtype wrappers for ergonomic `row.get::<_, _>(idx)` when a column is
//! stored as TEXT but should be decoded into a richer Rust type.

use cratestack_core::Value;
use rusqlite::types::{FromSql, FromSqlResult, ValueRef};

use super::decode::{decode_datetime, decode_decimal, decode_json, decode_uuid};

#[derive(Debug, Clone, Copy)]
pub struct UuidColumn(pub uuid::Uuid);

impl FromSql for UuidColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        decode_uuid(raw).map(UuidColumn)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DateTimeColumn(pub chrono::DateTime<chrono::Utc>);

impl FromSql for DateTimeColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        decode_datetime(raw).map(DateTimeColumn)
    }
}

#[derive(Debug, Clone)]
pub struct JsonColumn(pub Value);

impl FromSql for JsonColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        decode_json(raw).map(JsonColumn)
    }
}

#[derive(Debug, Clone)]
pub struct DecimalColumn(pub cratestack_core::Decimal);

impl FromSql for DecimalColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        decode_decimal(raw).map(DecimalColumn)
    }
}
