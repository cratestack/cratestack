//! `Value::to_plain_json` / `Value::from_plain_json` coverage
//! (cratestack#162): the untagged codec `Json` columns persist through,
//! as opposed to `Value`'s own derived, externally-tagged `Serialize`/
//! `Deserialize` used for wire/typed contexts elsewhere.

use std::collections::BTreeMap;

use serde_json::json;

use super::Value;

#[test]
fn empty_map_round_trips_as_plain_empty_object() {
    let value = Value::Map(BTreeMap::new());
    assert_eq!(value.to_plain_json(), json!({}));
    assert_eq!(Value::from_plain_json(json!({})), value);
}

#[test]
fn list_round_trips_as_plain_array() {
    let value = Value::List(vec![
        Value::String("a".to_owned()),
        Value::String("b".to_owned()),
    ]);
    assert_eq!(value.to_plain_json(), json!(["a", "b"]));
    assert_eq!(Value::from_plain_json(json!(["a", "b"])), value);
}

#[test]
fn null_round_trips_as_plain_null() {
    assert_eq!(Value::Null.to_plain_json(), serde_json::Value::Null);
    assert_eq!(Value::from_plain_json(serde_json::Value::Null), Value::Null);
}

#[test]
fn map_round_trips_as_plain_object() {
    let mut map = BTreeMap::new();
    map.insert("requests_per_second".to_owned(), Value::Int(5));
    let value = Value::Map(map);
    assert_eq!(value.to_plain_json(), json!({ "requests_per_second": 5 }));
    assert_eq!(
        Value::from_plain_json(json!({ "requests_per_second": 5 })),
        value
    );
}

#[test]
fn nested_structures_round_trip() {
    let mut inner = BTreeMap::new();
    inner.insert("nested".to_owned(), Value::Bool(true));
    let value = Value::List(vec![Value::Map(inner), Value::Float(1.5), Value::Null]);
    let plain = value.to_plain_json();
    assert_eq!(plain, json!([{ "nested": true }, 1.5, null]));
    assert_eq!(Value::from_plain_json(plain), value);
}

#[test]
fn legacy_plain_json_written_by_another_writer_decodes_cleanly() {
    // Exactly the shapes cratestack#162 reports as failing to decode
    // today: a bare empty object and a bare string array, neither of
    // which carry cratestack's own `{"Map": ...}` / `{"List": ...}` tag.
    assert_eq!(
        Value::from_plain_json(json!({})),
        Value::Map(BTreeMap::new())
    );
    assert_eq!(
        Value::from_plain_json(json!(["gpt-4", "gpt-4o"])),
        Value::List(vec![
            Value::String("gpt-4".to_owned()),
            Value::String("gpt-4o".to_owned()),
        ])
    );
}

#[test]
fn integral_floats_decode_as_int_not_float() {
    // `serde_json::Number::as_i64` succeeds for any JSON number written
    // without a fractional part/exponent, e.g. plain `5` — matching
    // Postgres's own jsonb number formatting for whole values.
    assert_eq!(Value::from_plain_json(json!(5)), Value::Int(5));
}

#[test]
fn bytes_round_trip_is_lossy_by_design() {
    // JSON has no byte-string type: `to_plain_json` base64-encodes, and
    // `from_plain_json` has no way to tell that string apart from an
    // ordinary one, so it comes back as `Value::String`, not
    // `Value::Bytes`. Documented on both methods — this test pins the
    // behavior rather than treating it as an oversight.
    let value = Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
    let plain = value.to_plain_json();
    assert_eq!(plain, json!("3q2+7w=="));
    assert_eq!(
        Value::from_plain_json(plain),
        Value::String("3q2+7w==".to_owned())
    );
}

#[test]
fn nan_float_falls_back_to_json_null() {
    assert_eq!(
        Value::Float(f64::NAN).to_plain_json(),
        serde_json::Value::Null
    );
}
