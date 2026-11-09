use cratestack_core::CratestackCodec;

use super::CborCodec;

#[test]
fn round_trips_value() {
    let codec = CborCodec;
    let bytes = codec
        .encode(&vec!["cool", "stack"])
        .expect("encode should succeed");
    let value: Vec<String> = codec.decode(&bytes).expect("decode should succeed");

    assert_eq!(value, vec!["cool".to_owned(), "stack".to_owned()]);
}

#[test]
fn encode_into_appends_exactly_what_encode_returns() {
    let codec = CborCodec;
    let value = (1_u32, "two", serde_json::Value::Null, vec![3_u8; 40]);
    let mut out = vec![0xd2, 0x84];
    codec.encode_into(&value, &mut out).expect("encode_into");
    assert_eq!(&out[..2], &[0xd2, 0x84]);
    assert_eq!(&out[2..], codec.encode(&value).expect("encode").as_slice());
}

struct Unserializable;

impl serde::Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("no"))
    }
}

#[test]
fn encode_into_leaves_out_at_its_old_length_on_error() {
    let mut out = vec![1, 2, 3];
    // Partially written before failing: the map head goes out first.
    let partial = std::collections::BTreeMap::from([("k", Unserializable)]);
    assert!(CborCodec.encode_into(&partial, &mut out).is_err());
    assert_eq!(out, [1, 2, 3]);
}

#[test]
fn repro_serde_json_null_mis_encodes_as_empty_array() {
    // cratestack#657 reproduction: `serde_json::Value::Null`'s
    // `Serialize` impl calls `serialize_unit()`, not
    // `serialize_none()`, so plain `minicbor_serde::to_vec` (no
    // `serialize_unit_as_null`) encodes it as the CBOR empty-array
    // marker `0x80`, not RFC 8949 null `0xf6`.
    let bytes = minicbor_serde::to_vec(&serde_json::Value::Null).expect("encode null");
    assert_eq!(bytes, vec![0x80]);
}

#[test]
fn optional_none_round_trips_as_cbor_null() {
    // minicbor-serde encodes `Option::<T>::None` as the CBOR null
    // marker (`0xf6`, RFC 8949 §3.3 simple-value 22) — which is what
    // we want, via `serialize_none()`.
    let codec = CborCodec;
    let bytes = codec.encode(&Option::<String>::None).expect("encode none");
    assert_eq!(bytes, vec![0xf6]);
    let decoded: Option<String> = codec.decode(&bytes).expect("decode none");
    assert!(decoded.is_none());
}

#[test]
fn json_null_round_trips_as_cbor_null_through_this_codec() {
    // cratestack#657 regression guard, scoped to this codec: unlike the
    // bare `minicbor_serde::to_vec` reproduction above,
    // `CborCodec::encode` enables `serialize_unit_as_null`, so
    // `serde_json::Value::Null` — which takes the `serialize_unit()`
    // path, not `serialize_none()` — now also encodes as `0xf6`. This
    // is the type the `/rpc/batch` envelope actually carries per frame
    // (`RpcRequest.input`/`RpcResponseFrame.output` are opaque
    // `serde_json::Value`, not `Option<T>`), so this is the case that
    // previously reached the wire as `0x80` on the batch path even
    // though `optional_none_round_trips_as_cbor_null` above was green.
    let codec = CborCodec;
    let bytes = codec
        .encode(&serde_json::Value::Null)
        .expect("encode json null");
    assert_eq!(bytes, vec![0xf6]);
    let decoded: serde_json::Value = codec.decode(&bytes).expect("decode json null");
    assert!(decoded.is_null());
}
