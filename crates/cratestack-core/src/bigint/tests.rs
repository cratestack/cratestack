//! `BigInt` unit tests that need no codec: the grammar and the checked
//! arithmetic. serde through `serde_json` is in `tests_json.rs` and the CBOR
//! bytes are pinned in `tests_cbor.rs`; the real `JsonCodec` and `CborCodec`
//! are exercised by the codec crates' `tests/bigint_codec.rs`.

use std::collections::HashSet;

use super::BigInt;

/// The five values the wire contract is pinned at, with their canonical text.
pub(super) const BOUNDARIES: [(i64, &str); 5] = [
    (i64::MAX, "9223372036854775807"),
    (i64::MIN, "-9223372036854775808"),
    (9_007_199_254_740_993, "9007199254740993"),
    (0, "0"),
    (-1, "-1"),
];

/// Strings that are not the canonical form, or are outside `i64`.
pub(super) const REFUSED_STRINGS: [&str; 20] = [
    "+5",
    "+0",
    "007",
    "00",
    "-0",
    " 1",
    "1 ",
    "\n1",
    "",
    "-",
    "--1",
    "0x10",
    "1e3",
    "1.0",
    "\u{ff11}\u{ff12}",
    "9223372036854775808",
    "-9223372036854775809",
    "99999999999999999999",
    "-99999999999999999999",
    "null",
];

#[test]
fn display_and_from_str_round_trip_the_boundaries() {
    for (value, text) in BOUNDARIES {
        let big = BigInt::new(value);
        assert_eq!(big.to_string(), text);
        assert_eq!(text.parse::<BigInt>(), Ok(big));
    }
}

#[test]
fn from_str_refuses_every_non_canonical_form() {
    for text in REFUSED_STRINGS {
        assert!(text.parse::<BigInt>().is_err(), "{text:?} must be refused");
    }
}

#[test]
fn from_str_error_says_which_kind_of_refusal() {
    let out_of_range = "9223372036854775808".parse::<BigInt>().unwrap_err();
    assert!(out_of_range.to_string().contains("64-bit range"));
    let leading_zero = "007".parse::<BigInt>().unwrap_err();
    assert!(leading_zero.to_string().contains("canonical"));
    // Neither message repeats the input.
    let echoed = "99999999999999999999".parse::<BigInt>().unwrap_err();
    assert!(!echoed.to_string().contains("99999999999999999999"));
}

#[test]
fn checked_operations_return_none_at_the_edges() {
    let (max, min, one) = (BigInt::MAX, BigInt::MIN, BigInt::new(1));
    assert_eq!(max.checked_add(one), None);
    assert_eq!(min.checked_sub(one), None);
    assert_eq!(max.checked_mul(BigInt::new(2)), None);
    assert_eq!(min.checked_mul(BigInt::new(-1)), None);
    assert_eq!(
        max.checked_add(BigInt::new(-1)),
        Some(BigInt::new(i64::MAX - 1))
    );
    assert_eq!(
        min.checked_sub(BigInt::new(-1)),
        Some(BigInt::new(i64::MIN + 1))
    );
    assert_eq!(
        BigInt::new(6).checked_mul(BigInt::new(7)),
        Some(BigInt::new(42))
    );
}

#[test]
fn conversions_constants_and_derives() {
    assert_eq!(BigInt::from(7_i64), BigInt::new(7));
    assert_eq!(i64::from(BigInt::new(-7)), -7);
    assert_eq!(BigInt::MAX.get(), i64::MAX);
    assert_eq!(BigInt::MIN.get(), i64::MIN);
    assert_eq!(BigInt::default(), BigInt::new(0));
    assert!(BigInt::new(-1) < BigInt::new(0));
    let copy = BigInt::new(5);
    let moved = copy;
    assert_eq!(copy, moved);
    let set: HashSet<BigInt> = [1, 1, 2].into_iter().map(BigInt::new).collect();
    assert_eq!(set.len(), 2);
}
