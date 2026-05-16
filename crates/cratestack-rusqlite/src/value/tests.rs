//! Round-trip tests across the binding/decoding boundary.

#![cfg(test)]

use super::*;
use cratestack_sql::SqlValue;
use rusqlite::Connection;

fn round_trip(value: SqlValue) -> SqlValue {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    // BLOB affinity preserves the storage class of the bound value
    // (NUMERIC affinity would coerce TEXT-numbers to INTEGER/REAL,
    // which would mask precision loss in the Decimal round-trip).
    conn.execute_batch("CREATE TABLE t (x BLOB)").unwrap();
    conn.execute("INSERT INTO t (x) VALUES (?1)", [SqlValueParam(&value)])
        .unwrap();
    // Read back as a raw rusqlite Value so we can inspect storage class.
    let storage: rusqlite::types::Value = conn
        .query_row("SELECT x FROM t", [], |row| row.get(0))
        .unwrap();
    match (value, storage) {
        (SqlValue::Bool(_), rusqlite::types::Value::Integer(i)) => SqlValue::Bool(i != 0),
        (SqlValue::Int(_), rusqlite::types::Value::Integer(i)) => SqlValue::Int(i),
        (SqlValue::Float(_), rusqlite::types::Value::Real(f)) => SqlValue::Float(f),
        (SqlValue::String(_), rusqlite::types::Value::Text(s)) => SqlValue::String(s),
        (SqlValue::Bytes(_), rusqlite::types::Value::Blob(b)) => SqlValue::Bytes(b),
        (SqlValue::Uuid(_), rusqlite::types::Value::Text(s)) => {
            SqlValue::Uuid(decode_uuid(&s).unwrap())
        }
        (SqlValue::DateTime(_), rusqlite::types::Value::Text(s)) => {
            SqlValue::DateTime(decode_datetime(&s).unwrap())
        }
        (SqlValue::Json(_), rusqlite::types::Value::Text(s)) => {
            SqlValue::Json(decode_json(&s).unwrap())
        }
        (SqlValue::Decimal(_), rusqlite::types::Value::Text(s)) => {
            SqlValue::Decimal(decode_decimal(&s).unwrap())
        }
        (v, rusqlite::types::Value::Null) if is_null_variant(&v) => v,
        (v, other) => panic!("unexpected round-trip: input {v:?} → storage {other:?}"),
    }
}

fn is_null_variant(v: &SqlValue) -> bool {
    matches!(
        v,
        SqlValue::NullBool
            | SqlValue::NullInt
            | SqlValue::NullFloat
            | SqlValue::NullString
            | SqlValue::NullBytes
            | SqlValue::NullUuid
            | SqlValue::NullDateTime
            | SqlValue::NullJson
            | SqlValue::NullDecimal
    )
}

#[test]
fn round_trips_bool() {
    assert_eq!(round_trip(SqlValue::Bool(true)), SqlValue::Bool(true));
    assert_eq!(round_trip(SqlValue::Bool(false)), SqlValue::Bool(false));
}

#[test]
fn round_trips_int() {
    assert_eq!(round_trip(SqlValue::Int(42)), SqlValue::Int(42));
    assert_eq!(round_trip(SqlValue::Int(-7)), SqlValue::Int(-7));
}

#[test]
fn round_trips_float() {
    assert_eq!(round_trip(SqlValue::Float(2.5)), SqlValue::Float(2.5));
}

#[test]
fn round_trips_string() {
    let s = SqlValue::String("hello world".to_owned());
    assert_eq!(round_trip(s.clone()), s);
}

#[test]
fn round_trips_bytes() {
    let b = SqlValue::Bytes(vec![0, 1, 2, 3, 254, 255]);
    assert_eq!(round_trip(b.clone()), b);
}

#[test]
fn round_trips_uuid_as_canonical_text() {
    let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    assert_eq!(round_trip(SqlValue::Uuid(id)), SqlValue::Uuid(id));
}

#[test]
fn round_trips_datetime_as_rfc3339_utc() {
    let dt = chrono::DateTime::parse_from_rfc3339("2026-05-11T12:34:56.789012Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(round_trip(SqlValue::DateTime(dt)), SqlValue::DateTime(dt));
}

#[test]
fn round_trips_json() {
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "k".to_string(),
        cratestack_core::Value::List(vec![
            cratestack_core::Value::Int(1),
            cratestack_core::Value::Int(2),
            cratestack_core::Value::Int(3),
        ]),
    );
    let v = SqlValue::Json(cratestack_core::Value::Map(map));
    assert_eq!(round_trip(v.clone()), v);
}

#[test]
fn round_trips_decimal_preserves_precision() {
    let d: cratestack_core::Decimal = "12345.67890".parse().unwrap();
    assert_eq!(round_trip(SqlValue::Decimal(d)), SqlValue::Decimal(d));
}

#[test]
fn round_trips_all_null_variants_as_storage_null() {
    for v in [
        SqlValue::NullBool,
        SqlValue::NullInt,
        SqlValue::NullFloat,
        SqlValue::NullString,
        SqlValue::NullBytes,
        SqlValue::NullUuid,
        SqlValue::NullDateTime,
        SqlValue::NullJson,
        SqlValue::NullDecimal,
    ] {
        assert_eq!(round_trip(v.clone()), v);
    }
}
