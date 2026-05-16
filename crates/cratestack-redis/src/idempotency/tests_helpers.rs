#![cfg(test)]

use redis::Value as RedisValue;

use super::parse::{next_bytes, next_i64_decimal, next_string, next_u16_decimal, value_as_string};
use super::tests_fixtures::raw_bulk;
use super::util::nibble_hex;

#[test]
fn nibble_hex_covers_all_valid_nibbles() {
    let table: Vec<(u8, char)> = (0u8..=15)
        .map(|n| {
            (
                n,
                match n {
                    0 => '0',
                    1 => '1',
                    2 => '2',
                    3 => '3',
                    4 => '4',
                    5 => '5',
                    6 => '6',
                    7 => '7',
                    8 => '8',
                    9 => '9',
                    10 => 'a',
                    11 => 'b',
                    12 => 'c',
                    13 => 'd',
                    14 => 'e',
                    15 => 'f',
                    _ => unreachable!(),
                },
            )
        })
        .collect();
    for (n, expected) in table {
        assert_eq!(nibble_hex(n), expected, "nibble {n}");
    }
}

#[test]
fn value_as_string_extracts_text_variants() {
    assert_eq!(
        value_as_string(&RedisValue::SimpleString("ok".into())).as_deref(),
        Some("ok"),
    );
    assert_eq!(
        value_as_string(&RedisValue::BulkString(b"hello".to_vec())).as_deref(),
        Some("hello"),
    );
    assert_eq!(value_as_string(&RedisValue::Okay).as_deref(), Some("OK"));
    // Non-textual variants return None so the caller has to handle
    // them explicitly rather than silently coercing.
    assert!(value_as_string(&RedisValue::Int(1)).is_none());
    assert!(value_as_string(&RedisValue::Nil).is_none());
}

#[test]
fn value_as_string_rejects_invalid_utf8_bulk_strings() {
    // Lone surrogate-style byte sequence isn't valid UTF-8. The
    // helper must report None rather than silently producing a
    // lossy string — otherwise we could mis-classify a `complete`
    // result.
    let bad = RedisValue::BulkString(vec![0xff, 0xfe, 0xfd]);
    assert!(value_as_string(&bad).is_none());
}

#[test]
fn next_i64_decimal_accepts_redis_int_directly() {
    // Redis returns Lua numbers as `Value::Int` rather than as a
    // decimal string, so the helper must accept both shapes.
    let mut iter = vec![RedisValue::Int(42)].into_iter();
    assert_eq!(next_i64_decimal(&mut iter, "x").unwrap(), 42);
}

#[test]
fn next_i64_decimal_accepts_simple_string() {
    let mut iter = vec![RedisValue::SimpleString("123".into())].into_iter();
    assert_eq!(next_i64_decimal(&mut iter, "x").unwrap(), 123);
}

#[test]
fn next_i64_decimal_rejects_garbage_bytes() {
    let mut iter = vec![raw_bulk(b"not-a-number")].into_iter();
    assert!(next_i64_decimal(&mut iter, "x").is_err());
}

#[test]
fn next_u16_decimal_rejects_negative() {
    let mut iter = vec![raw_bulk(b"-1")].into_iter();
    assert!(next_u16_decimal(&mut iter, "x").is_err());
}

#[test]
fn next_string_rejects_invalid_utf8() {
    let mut iter = vec![RedisValue::BulkString(vec![0xff, 0xfe])].into_iter();
    assert!(next_string(&mut iter, "x").is_err());
}

#[test]
fn next_bytes_treats_nil_as_empty() {
    // Redis returns Nil for unset hash fields. The replay parser
    // calls `next_bytes` on `response_headers`, which is allowed to
    // be empty — surfacing Nil as an error would break the empty-
    // headers case the integration tests rely on.
    let mut iter = vec![RedisValue::Nil].into_iter();
    assert_eq!(next_bytes(&mut iter, "x").unwrap(), Vec::<u8>::new());
}
