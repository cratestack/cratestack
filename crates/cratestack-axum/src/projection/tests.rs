//! `ProjectedValue` round-trip coverage — cratestack#430. Drives the
//! *real* `CborCodec`/`JsonCodec` (not a hand-rolled serializer stand-in)
//! so these tests fail the same way a live server response would if the
//! format-preserving leaf dispatch regressed.

use std::collections::BTreeMap;

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::CratestackCodec;
use serde::Deserialize;
use uuid::Uuid;

use super::ProjectedValue;

/// Mirrors a generated client's decode target for a model with one
/// `Uuid` column: a plain `uuid::Uuid` field, no wrapper type.
#[derive(Debug, Deserialize, PartialEq)]
struct WithUuid {
    id: Uuid,
}

/// Mirrors a model with two `Uuid` columns — the "every field, not just
/// the first" case the issue's "check for other instances" ask covers.
#[derive(Debug, Deserialize, PartialEq)]
struct WithTwoUuids {
    id: Uuid,
    #[serde(rename = "externalId")]
    external_id: Uuid,
}

/// Mirrors a model with a nullable `Uuid` column, generated client-side
/// with `#[serde(default)]` (see `struct_field_definition` in
/// `cratestack-macros`).
#[derive(Debug, Deserialize, PartialEq)]
struct WithNullableUuid {
    #[serde(default)]
    owner_id: Option<Uuid>,
}

fn object(fields: Vec<(&str, ProjectedValue)>) -> ProjectedValue {
    let map: BTreeMap<String, ProjectedValue> = fields
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    ProjectedValue::Object(map)
}

#[test]
fn uuid_leaf_round_trips_over_cbor() {
    let id = Uuid::new_v4();
    let value = object(vec![("id", ProjectedValue::leaf(id))]);

    let codec = CborCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    let decoded: WithUuid = codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(decoded, WithUuid { id });
}

#[test]
fn uuid_leaf_round_trips_over_json_without_regression() {
    let id = Uuid::new_v4();
    let value = object(vec![("id", ProjectedValue::leaf(id))]);

    let codec = JsonCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");

    // JSON must still carry the human-readable string form — this is
    // the "must not break the human-readable path" half of the fix.
    let text = String::from_utf8(bytes.clone()).expect("valid utf8");
    assert!(
        text.contains(&id.to_string()),
        "expected JSON body to contain the canonical Uuid string, got: {text}"
    );

    let decoded: WithUuid = codec.decode(&bytes).expect("decode should succeed");
    assert_eq!(decoded, WithUuid { id });
}

#[test]
fn multiple_uuid_columns_round_trip_over_cbor() {
    let id = Uuid::new_v4();
    let external_id = Uuid::new_v4();
    let value = object(vec![
        ("id", ProjectedValue::leaf(id)),
        ("externalId", ProjectedValue::leaf(external_id)),
    ]);

    let codec = CborCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    let decoded: WithTwoUuids = codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(decoded, WithTwoUuids { id, external_id });
}

#[test]
fn nullable_uuid_present_round_trips_over_cbor() {
    let owner_id = Uuid::new_v4();
    let value = object(vec![("ownerId", ProjectedValue::leaf(Some(owner_id)))]);

    let codec = CborCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    let decoded: BTreeMap<String, Option<Uuid>> =
        codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(decoded.get("ownerId"), Some(&Some(owner_id)));
}

#[test]
fn null_variant_round_trips_as_real_cbor_null_not_empty_array() {
    // This is the bug the old `serde_json::Value::Null` detour hit
    // (`serialize_unit()` under minicbor-serde encodes as an empty
    // array, not CBOR null) — see the module doc. `ProjectedValue::Null`
    // must use `serialize_none()` instead, so a client's
    // `Option<Uuid>` decodes `None`, not a decode error.
    let value = object(vec![("ownerId", ProjectedValue::Null)]);

    let codec = CborCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    let decoded: WithNullableUuid = codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(decoded, WithNullableUuid { owner_id: None });
}

#[test]
fn null_variant_round_trips_over_json() {
    let value = object(vec![("ownerId", ProjectedValue::Null)]);

    let codec = JsonCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    assert_eq!(
        String::from_utf8(bytes.clone()).expect("valid utf8"),
        "{\"ownerId\":null}"
    );

    let decoded: WithNullableUuid = codec.decode(&bytes).expect("decode should succeed");
    assert_eq!(decoded, WithNullableUuid { owner_id: None });
}

#[test]
fn array_of_objects_round_trips_over_cbor() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let value = ProjectedValue::Array(vec![
        object(vec![("id", ProjectedValue::leaf(first))]),
        object(vec![("id", ProjectedValue::leaf(second))]),
    ]);

    let codec = CborCodec;
    let bytes = codec.encode(&value).expect("encode should succeed");
    let decoded: Vec<WithUuid> = codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(
        decoded,
        vec![WithUuid { id: first }, WithUuid { id: second }]
    );
}

#[test]
fn non_uuid_leaves_still_round_trip_over_both_codecs() {
    let value = object(vec![
        ("name", ProjectedValue::leaf("cratestack".to_owned())),
        ("count", ProjectedValue::leaf(7_i64)),
        ("active", ProjectedValue::leaf(true)),
    ]);

    #[derive(Debug, Deserialize, PartialEq)]
    struct Plain {
        name: String,
        count: i64,
        active: bool,
    }
    let expected = Plain {
        name: "cratestack".to_owned(),
        count: 7,
        active: true,
    };

    let cbor = CborCodec;
    let cbor_bytes = cbor.encode(&value).expect("encode should succeed");
    let decoded: Plain = cbor.decode(&cbor_bytes).expect("decode should succeed");
    assert_eq!(decoded, expected);

    let json = JsonCodec;
    let value = object(vec![
        ("name", ProjectedValue::leaf("cratestack".to_owned())),
        ("count", ProjectedValue::leaf(7_i64)),
        ("active", ProjectedValue::leaf(true)),
    ]);
    let json_bytes = json.encode(&value).expect("encode should succeed");
    let decoded: Plain = json.decode(&json_bytes).expect("decode should succeed");
    assert_eq!(decoded, expected);
}
