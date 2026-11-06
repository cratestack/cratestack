//! Tests of [`FlutterCompactKeys`].

use cratestack_core::compact::{KeyNode, KeyStruct, KeyTables, OpKeys};

use super::*;

/// One op, `shop.put`, whose request is `{amount: 0, id: 1}` and whose
/// response is opaque.
fn tables() -> KeyTables {
    KeyTables {
        types: vec![KeyStruct {
            fields: vec![
                ("amount".to_owned(), KeyNode::Opaque),
                ("id".to_owned(), KeyNode::Opaque),
            ],
        }],
        ops: vec![OpKeys {
            op: "shop.put".to_owned(),
            digest: [7; 32],
            request: KeyNode::Struct(0),
            response: KeyNode::Opaque,
        }],
    }
}

fn keys() -> FlutterCompactKeys {
    FlutterCompactKeys::from_tables(tables().to_bytes()).expect("artifact loads")
}

/// The fixture artifact the JS suites hardcode
/// (`packages/cratestack-cbor-node/tests/compact.test.ts`) and the Dart tests (`dart-packages/cratestack_cbor/test/shared_fixtures.dart`).
const ARTIFACT_HEX: &str = "8301818466616d6f756e74006269640081846873686f702e707574582007070707070707070707070707070707070707070707070707070707070707070100";

#[test]
fn the_artifact_the_js_suites_hardcode_is_what_the_fixture_serialises_to() {
    let hex: String = tables()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, ARTIFACT_HEX);
}

// {"id":"a","amount":5} and its compact form {1:"a",0:5}.
const NAMED: [u8; 14] = [
    0xa2, 0x62, 0x69, 0x64, 0x61, 0x61, 0x66, 0x61, 0x6d, 0x6f, 0x75, 0x6e, 0x74, 0x05,
];
const COMPACT: [u8; 6] = [0xa2, 0x01, 0x61, 0x61, 0x00, 0x05];

fn cbor(
    f: impl Fn(&FlutterCompactKeys, Vec<u8>, String, String) -> Result<Vec<u8>, FlutterRuntimeError>,
    bytes: &[u8],
    op: &str,
    direction: &str,
) -> Result<Vec<u8>, FlutterRuntimeError> {
    f(&keys(), bytes.to_vec(), op.to_owned(), direction.to_owned())
}

#[test]
fn cbor_round_trips_through_the_single_entry_of_the_op() {
    let to = FlutterCompactKeys::to_compact;
    let from = FlutterCompactKeys::to_named;
    assert_eq!(cbor(to, &NAMED, "shop.put", "request").unwrap(), COMPACT);
    assert_eq!(cbor(from, &COMPACT, "shop.put", "request").unwrap(), NAMED);
    // The response tree is opaque: bytes pass through.
    assert_eq!(cbor(to, &NAMED, "shop.put", "response").unwrap(), NAMED);
}

#[test]
fn json_round_trips_with_decimal_string_keys() {
    let keys = keys();
    let named = r#"{"id":"a","amount":5}"#;
    let compact = r#"{"1":"a","0":5}"#;
    let call = |f: fn(
        &FlutterCompactKeys,
        String,
        String,
        String,
    ) -> Result<String, FlutterRuntimeError>,
                text: &str| {
        f(
            &keys,
            text.to_owned(),
            "shop.put".to_owned(),
            "request".to_owned(),
        )
    };
    assert_eq!(
        call(FlutterCompactKeys::to_compact_json, named).unwrap(),
        compact
    );
    assert_eq!(
        call(FlutterCompactKeys::to_named_json, compact).unwrap(),
        named
    );
}

#[test]
fn selector_is_the_eight_byte_digest_prefix_in_base64url() {
    // 0x07 * 8, unpadded base64url.
    assert_eq!(
        keys().selector("shop.put".to_owned()).as_deref(),
        Some("BwcHBwcHBwc")
    );
    assert_eq!(keys().selector("shop.none".to_owned()), None);
}

#[test]
fn bad_input_is_an_error_never_a_panic() {
    let to = FlutterCompactKeys::to_compact;
    let from = FlutterCompactKeys::to_named;
    assert!(cbor(to, &NAMED, "shop.none", "request").is_err());
    let error = cbor(to, &NAMED, "shop.put", "sideways").unwrap_err();
    assert!(error.message.contains("direction"), "{}", error.message);
    assert!(cbor(to, &[0x1b], "shop.put", "request").is_err());
    // An integer key past the table is refused on the way back.
    assert!(cbor(from, &[0xa1, 0x09, 0x00], "shop.put", "request").is_err());
    assert!(
        keys()
            .to_named_json("{".to_owned(), "shop.put".to_owned(), "request".to_owned())
            .is_err()
    );
    assert!(FlutterCompactKeys::from_tables(vec![1, 2, 3]).is_err());
}
