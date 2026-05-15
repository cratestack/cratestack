use super::*;

#[test]
fn length_rejects_below_min_and_above_max() {
    assert!(validate_length("name", "ab", Some(3), None).is_err());
    assert!(validate_length("name", "abcd", None, Some(3)).is_err());
    assert!(validate_length("name", "abc", Some(3), Some(3)).is_ok());
}

#[test]
fn email_accepts_simple_form_and_rejects_bad_shapes() {
    assert!(validate_email("e", "alice@example.com").is_ok());
    assert!(validate_email("e", "alice@example").is_err());
    assert!(validate_email("e", "alice@@example.com").is_err());
    assert!(validate_email("e", "alice example.com").is_err());
    assert!(validate_email("e", "@example.com").is_err());
}

#[test]
fn iso4217_requires_three_uppercase_letters() {
    assert!(validate_iso4217("currency", "USD").is_ok());
    assert!(validate_iso4217("currency", "usd").is_err());
    assert!(validate_iso4217("currency", "USDX").is_err());
    assert!(validate_iso4217("currency", "U1D").is_err());
}

#[test]
fn range_i64_enforces_inclusive_bounds() {
    assert!(validate_range_i64("n", 5, Some(0), Some(10)).is_ok());
    assert!(validate_range_i64("n", -1, Some(0), None).is_err());
    assert!(validate_range_i64("n", 11, None, Some(10)).is_err());
}

#[cfg(feature = "decimal-rust-decimal")]
#[test]
fn range_decimal_enforces_inclusive_bounds_after_promoting_i64_to_decimal() {
    use core::str::FromStr;
    let zero = Decimal::from(0);
    let mid = Decimal::from_str("4.5").unwrap();
    let just_below_zero = Decimal::from_str("-0.01").unwrap();
    let too_big = Decimal::from_str("10.01").unwrap();
    assert!(validate_range_decimal("amount", &zero, Some(0), Some(10)).is_ok());
    assert!(validate_range_decimal("amount", &mid, Some(0), Some(10)).is_ok());
    // Fractionally below the integer minimum still rejects — the
    // Decimal comparison must NOT silently round.
    assert!(validate_range_decimal("amount", &just_below_zero, Some(0), None).is_err());
    assert!(validate_range_decimal("amount", &too_big, None, Some(10)).is_err());
}

#[test]
fn validation_error_does_not_echo_value() {
    let err = validate_email("primary_email", "not-an-email").unwrap_err();
    let msg = err.public_message().into_owned();
    assert!(
        !msg.contains("not-an-email"),
        "validation message must not echo the rejected value: {msg}",
    );
}

#[cfg(feature = "decimal-rust-decimal")]
#[test]
fn decimal_alias_round_trips_through_json_as_string() {
    use std::str::FromStr;
    let value = Decimal::from_str("1234.56").unwrap();
    let encoded = serde_json::to_string(&value).unwrap();
    // `serde-str` makes Decimal serialize as a JSON string. Critical
    // so amounts never round-trip through f64.
    assert_eq!(encoded, "\"1234.56\"");
    let decoded: Decimal = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, value);
}

#[cfg(feature = "decimal-rust-decimal")]
#[test]
fn decimal_supports_precise_arithmetic() {
    use std::str::FromStr;
    // 0.1 + 0.2 — the canonical demonstration that f64 cannot
    // represent monetary arithmetic precisely.
    let a = Decimal::from_str("0.1").unwrap();
    let b = Decimal::from_str("0.2").unwrap();
    let sum = a + b;
    assert_eq!(sum, Decimal::from_str("0.3").unwrap());
}
