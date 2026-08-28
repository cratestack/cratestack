//! cratestack#783 regression test: every generated `Bytes` field shape
//! must decode a CBOR byte string (RFC 8949 major type 2) *and* the
//! integer array every already-deployed client sends.
//!
//! This runs against `CborCodec` — the real codec the server's
//! `decode_transport_request_for` / `decode_rpc_body` path uses — on the
//! actual macro-generated structs, so it exercises the emitted
//! `#[serde(deserialize_with = …)]` rather than the `cratestack-core`
//! helpers in isolation (`cratestack_core::lenient_bytes`'s own tests
//! cover those, and `cratestack-macros`' `bytes_serde` tests cover the
//! shape→helper mapping). The gap those two leave open is exactly this
//! one: whether the macro attaches the attribute to the right fields.
//!
//! Deliberately DB-free — decoding is a pure function of the generated
//! struct, so this never touches Postgres and never silently skips.

use cratestack::CratestackCodec;
use cratestack::include_server_schema;
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/bytes_wire_shapes.cstack", db = Postgres);

/// `0x44 01020304` — a 4-byte CBOR byte string. What `@cratestack/cbor`
/// emits for `new Uint8Array([1, 2, 3, 4])`.
const BYTE_STRING: [u8; 5] = [0x44, 0x01, 0x02, 0x03, 0x04];
/// `0x84 01 02 03 04` — the same four bytes as an array of integers.
/// What every generated Rust/Dart/TypeScript client emits today, and the
/// only shape JSON can express.
const INT_ARRAY: [u8; 5] = [0x84, 0x01, 0x02, 0x03, 0x04];
const EXPECTED: [u8; 4] = [1, 2, 3, 4];

/// Builds a CBOR map from `(key, raw payload)` pairs. Hand-assembled
/// rather than encoded from a Rust value on purpose: encoding a
/// `Vec<u8>` through the codec can only ever produce the array form, so
/// it could never express the byte-string case this test is about.
fn cbor_map(entries: &[(&str, &[u8])]) -> Vec<u8> {
    assert!(entries.len() < 24, "only short-form map headers are built");
    let mut bytes = vec![0xa0 | entries.len() as u8];
    for (key, payload) in entries {
        assert!(key.len() < 24, "only short-form text headers are built");
        bytes.push(0x60 | key.len() as u8);
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(payload);
    }
    bytes
}

/// A one-element CBOR array wrapping `payload`.
fn cbor_list(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x81];
    bytes.extend_from_slice(payload);
    bytes
}

fn decode<T: for<'de> serde::Deserialize<'de>>(bytes: &[u8]) -> T {
    CborCodec
        .decode(bytes)
        .unwrap_or_else(|error| panic!("decode should succeed: {error}"))
}

#[test]
fn procedure_args_accept_a_byte_string_for_every_arity() {
    // The exact shape cratestack#783 reports: a `Bytes` argument on a
    // procedure, which over `transport rpc` is what `POST
    // /rpc/procedure.sealMailbox` decodes its body into.
    let args: cratestack_schema::procedures::seal_mailbox::Args = decode(&cbor_map(&[
        ("payload", &BYTE_STRING),
        ("aad", &BYTE_STRING),
        ("chunks", &cbor_list(&BYTE_STRING)),
    ]));

    assert_eq!(args.payload, EXPECTED);
    assert_eq!(args.aad.as_deref(), Some(&EXPECTED[..]));
    assert_eq!(args.chunks, vec![EXPECTED.to_vec()]);
}

#[test]
fn procedure_args_still_accept_the_integer_array_every_client_sends() {
    let args: cratestack_schema::procedures::seal_mailbox::Args = decode(&cbor_map(&[
        ("payload", &INT_ARRAY),
        ("aad", &INT_ARRAY),
        ("chunks", &cbor_list(&INT_ARRAY)),
    ]));

    assert_eq!(args.payload, EXPECTED);
    assert_eq!(args.aad.as_deref(), Some(&EXPECTED[..]));
    assert_eq!(args.chunks, vec![EXPECTED.to_vec()]);
}

#[test]
fn an_omitted_nullable_argument_is_still_none() {
    // `deserialize_with` opts a field out of serde-derive's implicit
    // "missing `Option<T>` field defaults to `None`", so the generated
    // `#[serde(default, ...)]` has to carry this case. Without it, adding
    // byte-string leniency would have broken every caller that omits an
    // optional `Bytes` argument.
    let args: cratestack_schema::procedures::seal_mailbox::Args = decode(&cbor_map(&[
        ("payload", &BYTE_STRING),
        ("chunks", &cbor_list(&BYTE_STRING)),
    ]));

    assert_eq!(args.aad, None);
}

#[test]
fn type_block_fields_accept_both_shapes() {
    let from_byte_string: cratestack_schema::SealedEnvelope = decode(&cbor_map(&[
        ("nonce", &BYTE_STRING),
        ("aad", &BYTE_STRING),
        ("chunks", &cbor_list(&BYTE_STRING)),
    ]));
    let from_int_array: cratestack_schema::SealedEnvelope = decode(&cbor_map(&[
        ("nonce", &INT_ARRAY),
        ("aad", &INT_ARRAY),
        ("chunks", &cbor_list(&INT_ARRAY)),
    ]));

    assert_eq!(from_byte_string.nonce, EXPECTED);
    assert_eq!(from_byte_string.chunks, vec![EXPECTED.to_vec()]);
    assert_eq!(from_byte_string.nonce, from_int_array.nonce);
    assert_eq!(from_byte_string.aad, from_int_array.aad);
    assert_eq!(from_byte_string.chunks, from_int_array.chunks);
}

#[test]
fn model_and_create_input_accept_a_byte_string() {
    let model: cratestack_schema::Blob = decode(&cbor_map(&[
        ("id", &[0x01]),
        ("digest", &BYTE_STRING),
        ("signature", &BYTE_STRING),
    ]));
    assert_eq!(model.digest, EXPECTED);
    assert_eq!(model.signature.as_deref(), Some(&EXPECTED[..]));

    let input: cratestack_schema::CreateBlobInput = decode(&cbor_map(&[
        ("id", &[0x01]),
        ("digest", &BYTE_STRING),
        ("signature", &BYTE_STRING),
    ]));
    assert_eq!(input.digest, EXPECTED);
    assert_eq!(input.signature.as_deref(), Some(&EXPECTED[..]));
}

#[test]
fn patch_wrapped_update_input_keeps_its_three_way_distinction() {
    // `UpdateBlobInput` is the double-`Option` shape from cratestack#567:
    // `digest` is `Option<Vec<u8>>` (touched-or-not) and `signature` is
    // `Option<Option<Vec<u8>>>` (touched-or-not × set-or-cleared). Both
    // needed their own `Bytes` deserializer, and neither may lose the
    // distinction it already had.
    let touched: cratestack_schema::UpdateBlobInput = decode(&cbor_map(&[
        ("digest", &BYTE_STRING),
        ("signature", &BYTE_STRING),
    ]));
    assert_eq!(touched.digest.as_deref(), Some(&EXPECTED[..]));
    assert_eq!(touched.signature, Some(Some(EXPECTED.to_vec())));

    // `0xf6` — an explicit CBOR null, i.e. "clear this column".
    let cleared: cratestack_schema::UpdateBlobInput = decode(&cbor_map(&[("signature", &[0xf6])]));
    assert_eq!(cleared.digest, None);
    assert_eq!(cleared.signature, Some(None));

    // `0xa0` — an empty map: nothing touched at all.
    let untouched: cratestack_schema::UpdateBlobInput = decode(&[0xa0]);
    assert_eq!(untouched.digest, None);
    assert_eq!(untouched.signature, None);
}

#[test]
fn the_outbound_shape_is_unchanged() {
    // The fix is inbound-only, on purpose: flipping what `Serialize`
    // emits would break every existing decoder (the Dart client's
    // `cratestackAsValueList`, the TypeScript client's `number[]`). A
    // `Bytes` field must still go out as an integer array, never as a
    // byte string — see `cratestack_core::lenient_bytes`'s module doc.
    let input = cratestack_schema::CreateBlobInput {
        id: 1,
        digest: EXPECTED.to_vec(),
        signature: None,
    };
    let encoded = CborCodec.encode(&input).expect("encode should succeed");
    let hex: String = encoded.iter().map(|byte| format!("{byte:02x}")).collect();

    assert!(
        hex.contains("8401020304"),
        "digest must serialize as the 4-element array 0x84 01 02 03 04, got {hex}"
    );
    assert!(
        !hex.contains("4401020304"),
        "digest must NOT serialize as the byte string 0x44 01020304, got {hex}"
    );
}
