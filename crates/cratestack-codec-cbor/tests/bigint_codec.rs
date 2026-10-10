//! `BigInt` through the real `CborCodec` (ADR 0019 D2): a major type 3 text
//! string holding the canonical decimal form, never a CBOR integer or a
//! bignum tag, and a decode error whose `detail()` names the field.
//!
//! The expected bytes are written out as hex literals so an encoder regression
//! cannot move the expectation with it. `dart-packages/cratestack_cbor` and the
//! JS bridges pin the same bytes.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{BigInt, CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `{"amountE8": <value>}` as CBOR: map(1), text(8) "amountE8", then the value
/// as a major type 3 text string (`0x60 | length`, the ASCII digits).
const BOUNDARY_BYTES: [(i64, &str, &str); 5] = [
    (
        i64::MAX,
        "9223372036854775807",
        "a168616d6f756e7445387339323233333732303336383534373735383037",
    ),
    (
        i64::MIN,
        "-9223372036854775808",
        "a168616d6f756e744538742d39323233333732303336383534373735383038",
    ),
    (
        9_007_199_254_740_993,
        "9007199254740993",
        "a168616d6f756e7445387039303037313939323534373430393933",
    ),
    (0, "0", "a168616d6f756e7445386130"),
    (-1, "-1", "a168616d6f756e744538622d31"),
];

/// The head every hand-built refusal body starts with: map(1), "amountE8".
const AMOUNT_E8_HEAD: &str = "a168616d6f756e744538";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Amount {
    amount_e8: BigInt,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    amount_e8: BigInt,
    maybe: Option<BigInt>,
    list: Vec<BigInt>,
}

/// The same field as something that is not a `BigInt`, to build bodies the
/// `BigInt` type cannot produce.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Raw<T> {
    amount_e8: T,
}

fn row(value: i64) -> Row {
    Row {
        amount_e8: BigInt::new(value),
        maybe: Some(BigInt::new(value)),
        list: vec![BigInt::new(value)],
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn detail_of(result: Result<Amount, CratestackError>) -> String {
    let error = result.expect_err("the body must be refused");
    assert!(matches!(error, CratestackError::Codec(_)), "{error:?}");
    assert_eq!(error.public_message(), "invalid request payload");
    error
        .detail()
        .expect("a codec error has a detail")
        .to_owned()
}

fn assert_refused_naming_the_field(bytes: &[u8], what: &str) {
    let detail = detail_of(CborCodec.decode::<Amount>(bytes));
    assert!(
        detail.starts_with("failed to decode CBOR body: amountE8: "),
        "{what} ({}): {detail}",
        hex(bytes)
    );
}

#[test]
fn boundary_values_are_a_major_type_3_text_string_byte_for_byte() {
    for (value, text, expected) in BOUNDARY_BYTES {
        let amount = Amount {
            amount_e8: BigInt::new(value),
        };
        let bytes = CborCodec.encode(&amount).unwrap();
        assert_eq!(hex(&bytes), expected, "{value}");
        // The value item follows map(1) and the 9-byte key item.
        assert_eq!(bytes[10] >> 5, 3, "{value} is not major type 3");
        assert_eq!(&bytes[11..], text.as_bytes(), "{value}");
        assert_eq!(CborCodec.decode::<Amount>(&bytes).unwrap(), amount);
    }
}

#[test]
fn boundary_values_round_trip_in_optional_and_list_fields() {
    for (value, _, _) in BOUNDARY_BYTES {
        let bytes = CborCodec.encode(&row(value)).unwrap();
        assert_eq!(CborCodec.decode::<Row>(&bytes).unwrap(), row(value));
    }
}

#[test]
fn the_batch_path_keeps_the_same_bytes() {
    // `/rpc/batch` carries every frame as `serde_json::Value`: BigInt goes
    // struct -> Value (a JSON string) -> CBOR, and must land on the same bytes
    // the unary path writes.
    for (value, text, expected) in BOUNDARY_BYTES {
        let frame: Value = serde_json::to_value(Amount {
            amount_e8: BigInt::new(value),
        })
        .unwrap();
        assert_eq!(frame, json!({ "amountE8": text }));
        let bytes = CborCodec.encode(&frame).unwrap();
        assert_eq!(hex(&bytes), expected, "{value}");
        assert_eq!(CborCodec.decode::<Value>(&bytes).unwrap(), frame);

        let row_frame: Value = serde_json::to_value(row(value)).unwrap();
        let bytes = CborCodec.encode(&row_frame).unwrap();
        assert_eq!(CborCodec.decode::<Row>(&bytes).unwrap(), row(value));
    }
}

#[test]
fn a_cbor_integer_of_major_type_0_or_1_is_refused_and_the_field_is_named() {
    let positive = [
        0_u64,
        1,
        255,
        9_007_199_254_740_993,
        i64::MAX as u64,
        u64::MAX,
    ];
    for value in positive {
        let bytes = CborCodec.encode(&Raw { amount_e8: value }).unwrap();
        assert_eq!(bytes[10] >> 5, 0, "{value}");
        assert_refused_naming_the_field(&bytes, "major type 0");
    }
    for value in [-1_i64, -256, -9_007_199_254_740_993, i64::MIN] {
        let bytes = CborCodec.encode(&Raw { amount_e8: value }).unwrap();
        assert_eq!(bytes[10] >> 5, 1, "{value}");
        assert_refused_naming_the_field(&bytes, "major type 1");
    }
}

#[test]
fn a_cbor_bignum_tag_2_or_3_is_refused_and_the_field_is_named() {
    for (tagged, what) in [
        ("c2487fffffffffffffff", "tag 2, i64::MAX"),
        ("c24105", "tag 2, 5"),
        ("c3487ffffffffffffffe", "tag 3, -2^63 + 1"),
        ("c34100", "tag 3, -1"),
    ] {
        let bytes = unhex(&format!("{AMOUNT_E8_HEAD}{tagged}"));
        assert_refused_naming_the_field(&bytes, what);
    }
}

#[test]
fn other_cbor_items_are_refused_and_the_field_is_named() {
    for (item, what) in [
        ("f5", "true"),
        ("f6", "null"),
        ("fb3ff0000000000000", "1.0"),
        ("4101", "byte string"),
        ("80", "empty array"),
        ("a0", "empty map"),
        ("c06130", "text wrapped in tag 0"),
    ] {
        let bytes = unhex(&format!("{AMOUNT_E8_HEAD}{item}"));
        assert_refused_naming_the_field(&bytes, what);
    }
}

#[test]
fn every_non_canonical_text_string_is_refused_and_the_field_is_named() {
    for text in [
        "+5",
        "007",
        "-0",
        " 1",
        "1 ",
        "",
        "-",
        "9223372036854775808",
        "-9223372036854775809",
        "99999999999999999999",
    ] {
        let bytes = CborCodec.encode(&Raw { amount_e8: text }).unwrap();
        assert_eq!(bytes[10] >> 5, 3, "{text:?} is still a text string");
        assert_refused_naming_the_field(&bytes, text);
    }
}

#[test]
fn the_path_reaches_optional_and_list_fields() {
    let detail = |frame: Value| {
        let bytes = CborCodec.encode(&frame).unwrap();
        let error = CborCodec
            .decode::<Row>(&bytes)
            .expect_err("must be refused");
        error.detail().unwrap().to_owned()
    };
    let optional = detail(json!({"amountE8": "1", "maybe": 5, "list": []}));
    assert!(optional.contains("maybe: "), "{optional}");
    let list = detail(json!({"amountE8": "1", "maybe": null, "list": ["1", 2]}));
    assert!(list.contains("list[1]: "), "{list}");
    let list = detail(json!({"amountE8": "1", "maybe": null, "list": ["1", "007"]}));
    assert!(list.contains("list[1]: "), "{list}");
}

#[test]
fn a_bare_big_int_has_no_path_to_name() {
    let error = CborCodec.decode::<BigInt>(&unhex("05")).unwrap_err();
    let detail = error.detail().unwrap();
    assert!(
        detail.starts_with("failed to decode CBOR body: "),
        "{detail}"
    );
    assert!(!detail.contains("amountE8"), "{detail}");
    assert_eq!(
        CborCodec.decode::<BigInt>(&unhex("6135")).unwrap(),
        BigInt::new(5)
    );
}
