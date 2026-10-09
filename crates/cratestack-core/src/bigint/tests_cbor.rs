//! `BigInt` on CBOR, against `minicbor-serde` directly (core cannot dev-depend
//! on `cratestack-codec-cbor`, which depends on core). The bytes are written
//! out by hand so a regression in the encoder cannot move the expectation with
//! it. The same bytes are asserted through the real `CborCodec` in
//! `crates/cratestack-codec-cbor/tests/bigint_codec.rs`.

use super::BigInt;
use super::tests::{BOUNDARIES, REFUSED_STRINGS};

/// Major type 3 (`0x60 | length`), then the ASCII digits.
const BOUNDARY_BYTES: [(i64, &str); 5] = [
    (i64::MAX, "7339323233333732303336383534373735383037"),
    (i64::MIN, "742d39323233333732303336383534373735383038"),
    (9_007_199_254_740_993, "7039303037313939323534373430393933"),
    (0, "6130"),
    (-1, "622d31"),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn boundary_values_are_a_major_type_3_text_string() {
    for (value, expected) in BOUNDARY_BYTES {
        let bytes = minicbor_serde::to_vec(BigInt::new(value)).unwrap();
        assert_eq!(hex(&bytes), expected, "{value}");
        assert_eq!(bytes[0] >> 5, 3, "{value} is not major type 3");
    }
}

#[test]
fn the_text_is_the_canonical_decimal_form() {
    for (value, text) in BOUNDARIES {
        let bytes = minicbor_serde::to_vec(BigInt::new(value)).unwrap();
        assert_eq!(&bytes[1..], text.as_bytes(), "{value}");
    }
}

#[test]
fn boundary_values_round_trip() {
    for (value, expected) in BOUNDARY_BYTES {
        let decoded: BigInt = minicbor_serde::from_slice(&unhex(expected)).unwrap();
        assert_eq!(decoded, BigInt::new(value));
    }
}

#[test]
fn a_cbor_integer_of_major_type_0_or_1_is_refused() {
    // Even the exact value a native-integer encoding would have used.
    for value in [
        0_i64,
        1,
        255,
        9_007_199_254_740_993,
        i64::MAX,
        -1,
        -256,
        i64::MIN,
    ] {
        let bytes = minicbor_serde::to_vec(value).unwrap();
        assert!(
            matches!(bytes[0] >> 5, 0 | 1),
            "{value} is not major type 0 or 1"
        );
        assert!(
            minicbor_serde::from_slice::<BigInt>(&bytes).is_err(),
            "CBOR integer {value} must be refused"
        );
    }
}

#[test]
fn a_cbor_bignum_tag_2_or_3_is_refused() {
    for bytes in [
        // tag 2, byte string 0x7fffffffffffffff
        unhex("c2487fffffffffffffff"),
        // tag 2, byte string 0x05
        unhex("c24105"),
        // tag 3, byte string 0x7ffffffffffffffe (-2^63 + 1)
        unhex("c3487ffffffffffffffe"),
        // tag 3, byte string 0x00 (-1)
        unhex("c34100"),
    ] {
        assert!(
            minicbor_serde::from_slice::<BigInt>(&bytes).is_err(),
            "{} must be refused",
            hex(&bytes)
        );
    }
}

#[test]
fn other_cbor_items_are_refused() {
    for bytes in [
        unhex("f5"),                 // true
        unhex("f6"),                 // null
        unhex("fb3ff0000000000000"), // 1.0 as a double
        unhex("4101"),               // byte string 0x01
        unhex("80"),                 // empty array
        unhex("a0"),                 // empty map
        unhex("c06130"),             // text "0" wrapped in tag 0
    ] {
        assert!(
            minicbor_serde::from_slice::<BigInt>(&bytes).is_err(),
            "{} must be refused",
            hex(&bytes)
        );
    }
}

#[test]
fn every_non_canonical_text_string_is_refused() {
    for text in REFUSED_STRINGS {
        let bytes = minicbor_serde::to_vec(text).unwrap();
        assert!(
            minicbor_serde::from_slice::<BigInt>(&bytes).is_err(),
            "{text:?} must be refused"
        );
    }
}
