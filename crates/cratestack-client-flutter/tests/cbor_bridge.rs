//! Round-trip tests: `cratestack_client_flutter::cbor`'s JSON-mediated
//! bridge against `cratestack-codec-cbor`'s `CborCodec`, across the
//! scalar matrix a generated client's model actually carries
//! (cratestack#563).
//!
//! Two different, deliberately-not-conflated claims are checked:
//!
//! 1. **Byte-identical for a given JSON value.** The bridge's
//!    `encode_json`/`decode_json` do nothing but marshal JSON text at the
//!    boundary — the actual CBOR encoding is `CborCodec.encode(&value)`
//!    (see `cbor::tests::fixture_bytes_shared_with_the_napi_and_wasm_
//!    cross_language_tests_stay_correct` in `src/cbor/mod.rs`, which
//!    reuses the exact hex fixtures `cratestack-cbor-napi` and
//!    `cratestack-cbor-wasm` already assert byte-identical bytes for).
//!    `decimal_scalar_round_trips_as_a_json_string_and_matches_direct_
//!    codec_bytes` below extends that claim to `Decimal` specifically:
//!    `rust_decimal`'s `serde-str` feature makes `Decimal` unconditionally
//!    serialize as a string (it does not branch on `is_human_readable()`
//!    the way `Uuid` does — see point 2), so its bridge encoding stays
//!    byte-identical to `CborCodec` encoding a native `Decimal` field
//!    directly. `crates/cratestack-client-dart/src/wire_encode.rs` (the
//!    Dart generator's own scalar table) confirms `Uuid`/`Cuid`/`Decimal`
//!    all already cross the wire as a Dart `String` today — this bridge's
//!    JSON-string representation matches that existing convention.
//!
//! 2. **Interoperable, not necessarily byte-identical, for a realistic
//!    multi-field payload.** A CBOR *map*'s key order carries no meaning
//!    (decoding is name-keyed, not positional), so comparing raw bytes
//!    between two differently-shaped encoders — a struct serialized by
//!    field-declaration order versus a `serde_json::Value` object
//!    serialized by its own (unordered-map) key order — would assert an
//!    implementation detail neither side promises.
//!    `scalar_matrix_bridge_output_decodes_into_the_native_typed_struct`
//!    instead proves the thing that actually matters: bytes this bridge
//!    produces from a Dart-shaped JSON payload decode, field-for-field,
//!    into the native Rust struct types (`DateTime<Utc>`, `Decimal`) a
//!    generated Rust client/server would use for the same scalars.
//!
//! `Uuid` gets its own dedicated, narrower test:
//! `uuid_scalar_diverges_from_a_native_uuid_typed_struct_encoding`.
//! `uuid::Uuid::serialize` branches on `Serializer::is_human_readable()`:
//! `CborCodec` reports `false`, so `CborCodec` encoding a *native*
//! `Uuid`-typed Rust struct directly takes the compact 16-byte binary CBOR
//! branch — but this bridge's boundary type is JSON text, and
//! `serde_json` unconditionally reports `is_human_readable() == true`, so
//! the same logical value takes the 36-character hyphenated-string branch
//! instead (bytes produced by the two are not just non-identical but
//! mutually non-decodable — see the test). This is a real, deliberately
//! surfaced limitation of comparing against a *native-Rust-struct*
//! encoding specifically; it does not claim anything about what bytes a
//! generated client's own schema-aware wire layer puts on the CBOR wire
//! today (out of scope to trace fully here) — only that this generic
//! bridge and a raw `#[derive(Serialize)]` struct with a `Uuid` field are
//! not wire-compatible with each other. Same root cause as
//! `cratestack_core::Value::to_plain_json`'s own documented `Bytes` ->
//! base64-string tradeoff: a JSON-shaped boundary carries no format hint,
//! so it always takes the human-readable branch. Worth revisiting when
//! the generator seam (explicitly out of scope for this PR) is designed.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use cratestack_client_flutter::cbor;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{BigInt, CratestackCodec, Decimal};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct NestedObject {
    key: String,
    value: i64,
}

/// One field per scalar kind a generated CrateStack client model carries
/// that this generic bridge decodes into its native Rust type, minus
/// `Bytes`/`Json` — this bridge, like its napi/wasm siblings, does not
/// attempt a compact wire form for those — and minus `Uuid`, whose own
/// comparison needs a native `Uuid`-typed struct rather than a `String`
/// field; see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ScalarMatrix {
    text: String,
    id: String,
    integer: i64,
    float: f64,
    boolean: bool,
    optional_present: Option<String>,
    optional_absent: Option<String>,
    list: Vec<i64>,
    nested: NestedObject,
    created_at: DateTime<Utc>,
    amount: Decimal,
}

fn fixture() -> ScalarMatrix {
    ScalarMatrix {
        text: "cratestack".to_owned(),
        id: Uuid::parse_str("11111111-2222-3333-4444-555555555555")
            .unwrap()
            .to_string(),
        integer: 42,
        float: 3.5,
        boolean: true,
        optional_present: Some("present".to_owned()),
        optional_absent: None,
        list: vec![1, 2, 3],
        nested: NestedObject {
            key: "nested".to_owned(),
            value: 7,
        },
        created_at: DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        amount: Decimal::from_str("1234.5600").unwrap(),
    }
}

#[test]
fn scalar_matrix_bridge_output_decodes_into_the_native_typed_struct() {
    // Dart's `jsonEncode(model.toJson())` shape: every scalar already
    // rendered as its Dart-generator-conventional JSON representation
    // (`id`/`Decimal` as strings — see the module doc comment). This is
    // what actually flows into `encodeJson` from a generated client.
    let fixture = fixture();
    let json_text = serde_json::to_string(&fixture).expect("serialize fixture to JSON");

    let bytes = cbor::encode_json(json_text).expect("bridge encode");

    // The proof that matters: a native, schema-typed Rust struct
    // (`DateTime<Utc>`, `Decimal` — not `serde_json::Value`) decodes
    // these bytes correctly, field-for-field, independent of the map key
    // order the bridge happened to emit.
    let decoded: ScalarMatrix = CborCodec.decode(&bytes).expect("direct-codec decode");
    assert_eq!(decoded, fixture);

    // And the round trip through the bridge's own decode side agrees.
    let decoded_json = cbor::decode_json(bytes).expect("bridge decode");
    let decoded_via_bridge: ScalarMatrix =
        serde_json::from_str(&decoded_json).expect("deserialize decoded JSON");
    assert_eq!(decoded_via_bridge, fixture);
}

#[test]
fn decimal_scalar_round_trips_as_a_json_string_and_matches_direct_codec_bytes() {
    // Isolated single-field check for the scalar this ticket calls out by
    // name. A single-field struct sidesteps the map-key-order caveat
    // above entirely (one key, one possible order), so this is a genuine
    // byte-identical claim, not just an interop one.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct DecimalOnly {
        amount: Decimal,
    }

    let fixture = DecimalOnly {
        amount: Decimal::from_str("99999999999999.99").unwrap(),
    };
    let reference_bytes = CborCodec.encode(&fixture).expect("direct encode");

    let json_value =
        serde_json::to_value(&fixture).expect("serialize fixture to serde_json::Value");
    assert_eq!(
        json_value["amount"],
        json!(fixture.amount.to_string()),
        "Decimal must serialize to a plain JSON string over this bridge, matching Dart's \
         String representation (see crates/cratestack-client-dart/src/wire_encode.rs)"
    );

    let json_text = serde_json::to_string(&fixture).unwrap();
    let candidate_bytes = cbor::encode_json(json_text).expect("bridge encode");
    assert_eq!(
        candidate_bytes, reference_bytes,
        "Decimal must round-trip byte-identical to cratestack-codec-cbor's direct encoding"
    );

    let decoded_json = cbor::decode_json(reference_bytes).expect("bridge decode");
    let decoded: DecimalOnly = serde_json::from_str(&decoded_json).expect("deserialize");
    assert_eq!(decoded, fixture);
}

/// ADR 0019 D2: a `BigInt` travels as a CBOR text string (major type 3)
/// holding the canonical decimal form. Tuples are
/// `(decimal, exact CBOR hex of {"amountE8": <decimal>})`, the same five
/// strings as the other copies of this table (see the doc comment on
/// `BIGINT_FIXTURES` in `crates/cratestack-cbor-napi/src/lib.rs` for where
/// each lives and how the hex was derived by hand from RFC 8949).
const BIGINT_FIXTURES: [(&str, &str); 5] = [
    (
        "9223372036854775807",
        "a168616d6f756e7445387339323233333732303336383534373735383037",
    ),
    (
        "-9223372036854775808",
        "a168616d6f756e744538742d39323233333732303336383534373735383038",
    ),
    (
        "9007199254740993",
        "a168616d6f756e7445387039303037313939323534373430393933",
    ),
    ("0", "a168616d6f756e7445386130"),
    ("-1", "a168616d6f756e744538622d31"),
];

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
        .collect()
}

#[test]
fn bigint_text_string_round_trips_exactly_and_matches_the_fixture_bytes() {
    // Same single-field shape as the `Decimal` case above, so the
    // byte-identical claim is genuine (one key, one possible order). The
    // field is a plain `String` here on purpose: this is the bridge's
    // claim, independent of the `BigInt` newtype (checked in the next
    // test), and it is the shape a Dart `BigInt.toString()` hands over.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct AmountOnly {
        #[serde(rename = "amountE8")]
        amount_e8: String,
    }

    for (decimal, expected_hex) in BIGINT_FIXTURES {
        let expected = unhex(expected_hex);
        let fixture = AmountOnly {
            amount_e8: decimal.to_owned(),
        };

        let json_text = serde_json::to_string(&fixture).unwrap();
        assert_eq!(json_text, format!(r#"{{"amountE8":"{decimal}"}}"#));

        let bridged = cbor::encode_json(json_text.clone()).expect("bridge encode");
        assert_eq!(bridged, expected, "bridge encode {decimal}");
        assert_eq!(
            bridged,
            CborCodec.encode(&fixture).expect("direct encode"),
            "bridge must match the direct codec byte for byte for {decimal}"
        );
        assert_eq!(bridged[10] >> 5, 3, "{decimal} must be a text string");

        let decoded: AmountOnly = CborCodec.decode(&expected).expect("direct decode");
        assert_eq!(decoded, fixture, "direct decode {decimal}");

        // And back out through the bridge: the same JSON text, with the
        // value still a JSON string, never a number.
        let decoded_json = cbor::decode_json(expected).expect("bridge decode");
        assert_eq!(decoded_json, json_text, "bridge decode {decimal}");
        let parsed: serde_json::Value = serde_json::from_str(&decoded_json).unwrap();
        assert!(
            parsed["amountE8"].is_string(),
            "{decimal} must stay a string"
        );
    }
}

#[test]
fn bigint_native_type_encodes_to_the_fixture_bytes_the_bridge_carries() {
    // The native-typed half of the claim: a Rust struct with a real
    // `cratestack_core::BigInt` field encodes (through `CborCodec`) to the
    // exact bytes the bridge and every other binding assert, and the
    // bridge's output decodes back into it. `cratestack-core`'s own tests
    // pin the encoding byte for byte; this binds the bridge to it.
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    struct BigIntOnly {
        #[serde(rename = "amountE8")]
        amount_e8: BigInt,
    }

    for (decimal, expected_hex) in BIGINT_FIXTURES {
        let expected = unhex(expected_hex);
        let fixture = BigIntOnly {
            amount_e8: BigInt::new(decimal.parse::<i64>().expect("fixture fits i64")),
        };

        assert_eq!(
            CborCodec.encode(&fixture).expect("direct encode"),
            expected,
            "native BigInt encode {decimal}"
        );

        let json_text = serde_json::to_string(&fixture).unwrap();
        assert_eq!(json_text, format!(r#"{{"amountE8":"{decimal}"}}"#));
        let bridged = cbor::encode_json(json_text).expect("bridge encode");
        assert_eq!(bridged, expected, "bridge encode {decimal}");

        let decoded: BigIntOnly = CborCodec.decode(&bridged).expect("direct decode");
        assert_eq!(decoded, fixture, "native BigInt decode {decimal}");
    }
}

#[test]
fn bigint_text_string_and_native_integer_stay_distinct_through_the_bridge() {
    // The bridge carries whatever JSON it is handed, so a JSON number
    // reaches the wire as a CBOR integer (major type 0) and a JSON string
    // as a text string (major type 3). They must never be conflated: that
    // is what lets the Rust codec refuse a number at a `BigInt` key
    // (`cratestack-core`'s tests) instead of the bridge guessing.
    let as_number =
        cbor::encode_json(r#"{"amountE8":9223372036854775807}"#.to_owned()).expect("bridge encode");
    assert_eq!(
        as_number,
        unhex("a168616d6f756e7445381b7fffffffffffffff"),
        "a JSON number is a CBOR unsigned integer (0x1b, 8 bytes)"
    );
    assert_ne!(as_number, unhex(BIGINT_FIXTURES[0].1));

    let decoded_number = cbor::decode_json(as_number).expect("bridge decode");
    assert_eq!(decoded_number, r#"{"amountE8":9223372036854775807}"#);
    let decoded_string = cbor::decode_json(unhex(BIGINT_FIXTURES[0].1)).expect("bridge decode");
    assert_eq!(decoded_string, r#"{"amountE8":"9223372036854775807"}"#);
}

#[test]
fn uuid_scalar_diverges_from_a_native_uuid_typed_struct_encoding() {
    // See the module doc comment for the full `is_human_readable()`
    // explanation and its scope. This test asserts the divergence
    // explicitly, including that the two encodings are mutually
    // non-decodable, rather than silently passing or being silently
    // skipped — so a future change to either side has a test to fail
    // loudly against.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct UuidOnly {
        id: Uuid,
    }

    let fixture = UuidOnly {
        id: Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
    };
    let reference_bytes = CborCodec.encode(&fixture).expect("direct encode");

    let json_text = serde_json::to_string(&fixture).unwrap();
    let bridged_bytes = cbor::encode_json(json_text).expect("bridge encode");

    assert_ne!(
        bridged_bytes, reference_bytes,
        "Uuid is NOT expected to be byte-identical to a native-Uuid-typed struct encoding \
         over this bridge — see the module doc comment for why"
    );
    assert!(
        CborCodec.decode::<UuidOnly>(&bridged_bytes).is_err(),
        "bridge-encoded bytes must NOT decode into a native Uuid-typed struct — the bridge \
         took the CBOR text-string branch, not the compact binary-string branch CborCodec's \
         direct Uuid decode expects"
    );

    // The *value* still round-trips exactly through the bridge itself.
    let decoded_json = cbor::decode_json(bridged_bytes).expect("bridge decode");
    let decoded: UuidOnly = serde_json::from_str(&decoded_json).expect("deserialize");
    assert_eq!(decoded, fixture);
}

#[test]
fn top_level_and_nested_null_round_trip_as_cbor_null_through_the_bridge() {
    let original = json!({"a": null, "b": [1, null, "x"], "c": null});
    let json_text = serde_json::to_string(&original).unwrap();

    let bytes = cbor::encode_json(json_text).expect("encode");
    assert!(
        bytes.contains(&0xf6),
        "JSON null must encode as CBOR null (0xf6) somewhere in the payload, not as 0x80"
    );

    let decoded_json = cbor::decode_json(bytes).expect("decode");
    let decoded: serde_json::Value = serde_json::from_str(&decoded_json).unwrap();
    assert_eq!(decoded, original);
}
