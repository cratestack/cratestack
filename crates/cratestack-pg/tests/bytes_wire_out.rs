//! cratestack#1425: every generated `Bytes` field shape is *written* as a CBOR
//! byte string (RFC 8949 major type 2) under the real `CborCodec`, and JSON is
//! byte-identical to plain serde (an array of integers). The inbound half is
//! `bytes_wire_shapes.rs`; this is its outbound mirror, on the same fixture.
//!
//! DB-free: encoding is a pure function of the generated struct.

use cratestack::CratestackCodec;
use cratestack::include_server_schema;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use serde::Serialize;

include_server_schema!("tests/fixtures/bytes_wire_shapes.cstack", db = Postgres);

fn cbor<T: Serialize>(value: &T) -> String {
    let encoded = CborCodec.encode(value).expect("cbor encode");
    encoded.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json<T: Serialize>(value: &T) -> String {
    String::from_utf8(JsonCodec.encode(value).expect("json encode")).unwrap()
}

#[test]
fn type_block_fields_are_byte_strings_for_every_arity() {
    let value = cratestack_schema::SealedEnvelope {
        nonce: vec![1, 2, 255],
        aad: Some(vec![1]),
        chunks: vec![vec![1], vec![]],
    };
    // {"nonce": h'0102ff', "aad": h'01', "chunks": [h'01', h'']}
    assert_eq!(
        cbor(&value),
        "a3656e6f6e6365430102ff636161644101666368756e6b7382410140"
    );
    assert_eq!(
        json(&value),
        r#"{"nonce":[1,2,255],"aad":[1],"chunks":[[1],[]]}"#
    );

    let none = cratestack_schema::SealedEnvelope {
        nonce: vec![],
        aad: None,
        chunks: vec![],
    };
    // aad stays CBOR null (0xf6); an empty value is the empty byte string 0x40.
    assert_eq!(cbor(&none), "a3656e6f6e63654063616164f6666368756e6b7380");
}

#[test]
fn procedure_args_are_byte_strings() {
    let args = cratestack_schema::procedures::seal_mailbox::Args {
        payload: vec![1, 2, 255],
        aad: None,
        chunks: vec![vec![1]],
    };
    // {"payload": h'0102ff', "aad": null, "chunks": [h'01']}
    assert_eq!(
        cbor(&args),
        "a3677061796c6f6164430102ff63616164f6666368756e6b73814101"
    );
    assert_eq!(
        json(&args),
        r#"{"payload":[1,2,255],"aad":null,"chunks":[[1]]}"#
    );
}

#[test]
fn model_and_create_input_are_byte_strings() {
    let create = cratestack_schema::CreateBlobInput {
        id: 1,
        digest: vec![1, 2, 255],
        signature: None,
    };
    // {"id": 1, "digest": h'0102ff', "signature": null}
    assert_eq!(
        cbor(&create),
        "a36269640166646967657374430102ff697369676e6174757265f6"
    );
    assert_eq!(
        json(&create),
        r#"{"id":1,"digest":[1,2,255],"signature":null}"#
    );
}

#[test]
fn update_patch_shapes_keep_untouched_cleared_and_set_distinct() {
    let untouched = cratestack_schema::UpdateBlobInput::default();
    assert_eq!(cbor(&untouched), "a0");
    assert_eq!(json(&untouched), "{}");

    let cleared = cratestack_schema::UpdateBlobInput {
        signature: Some(None),
        ..Default::default()
    };
    // {"signature": null}: `Some(None)` is still null, never an empty byte string.
    assert_eq!(cbor(&cleared), "a1697369676e6174757265f6");
    assert_eq!(json(&cleared), r#"{"signature":null}"#);

    let set = cratestack_schema::UpdateBlobInput {
        digest: Some(vec![1, 2, 255]),
        signature: Some(Some(vec![9])),
        ..Default::default()
    };
    // {"digest": h'0102ff', "signature": h'09'}
    assert_eq!(
        cbor(&set),
        "a266646967657374430102ff697369676e61747572654109"
    );
    assert_eq!(json(&set), r#"{"digest":[1,2,255],"signature":[9]}"#);
}

#[test]
fn what_the_codec_writes_the_lenient_decoder_reads_back() {
    let input = cratestack_schema::CreateBlobInput {
        id: 7,
        digest: (0..=255u8).collect(),
        signature: Some(vec![]),
    };
    let encoded = CborCodec.encode(&input).unwrap();
    // 256 bytes: major type 2 with a 2-byte length (`59 0100`), not 256 array items.
    assert!(cbor(&input).contains("590100000102"));
    let back: cratestack_schema::CreateBlobInput = CborCodec.decode(&encoded).unwrap();
    assert_eq!(back.digest, input.digest);
    assert_eq!(back.signature, Some(vec![]));
}
