//! `BigInt` payload validation (ADR 0019): canonical decimal strings in,
//! JSON numbers and every non-canonical spelling out, and `@range`
//! actually checked rather than skipped.

use super::{ValidationCode, validate_payload};

const LEDGER: &str = r#"
    model Ledger {
      id String @id
      amountE8 BigInt
      capped BigInt @range(min: 0, max: 100)
      note String?
    }
"#;

fn parse(text: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(text).expect("schema parses")
}

fn payload(items: &[(&str, serde_json::Value)]) -> serde_json::Map<String, serde_json::Value> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// Validate a full create payload with `amount` as `amountE8`.
fn errors_for_amount(amount: serde_json::Value) -> Vec<super::FieldError> {
    let schema = parse(LEDGER);
    let model = schema.models.iter().find(|m| m.name == "Ledger").unwrap();
    validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("l1")),
            ("amountE8", amount),
            ("capped", serde_json::json!("1")),
        ]),
        false,
    )
}

#[test]
fn canonical_strings_are_accepted_including_both_i64_bounds() {
    for text in [
        "0",
        "-1",
        "9007199254740993",
        "9223372036854775807",
        "-9223372036854775808",
    ] {
        let errors = errors_for_amount(serde_json::json!(text));
        assert!(errors.is_empty(), "{text} should be accepted: {errors:?}");
    }
}

#[test]
fn a_json_number_is_refused_naming_the_field() {
    for number in [serde_json::json!(42), serde_json::json!(1.5)] {
        let errors = errors_for_amount(number.clone());
        let error = errors
            .iter()
            .find(|e| e.field == "amountE8")
            .unwrap_or_else(|| panic!("{number} should be refused: {errors:?}"));
        assert_eq!(error.code, ValidationCode::TypeMismatch);
        assert!(error.message.contains("amountE8"), "{}", error.message);
        assert!(
            error.message.contains("decimal string"),
            "{}",
            error.message
        );
    }
}

#[test]
fn non_canonical_and_out_of_range_strings_are_refused() {
    for text in [
        "+5",
        "007",
        "-0",
        " 1",
        "1 ",
        "",
        "1.0",
        "abc",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let errors = errors_for_amount(serde_json::json!(text));
        assert!(
            errors
                .iter()
                .any(|e| e.field == "amountE8" && e.code == ValidationCode::TypeMismatch),
            "{text:?} should be refused: {errors:?}"
        );
    }
}

#[test]
fn bool_and_array_values_are_refused() {
    for value in [serde_json::json!(true), serde_json::json!(["1"])] {
        let errors = errors_for_amount(value.clone());
        assert!(
            errors.iter().any(|e| e.field == "amountE8"),
            "{value} should be refused: {errors:?}"
        );
    }
}

/// `@range` on a `BigInt` reads a decimal string, which `as_i64` and
/// `as_f64` both return `None` for. The check used to bail out on that
/// `None`, which would have accepted every value silently.
#[test]
fn range_is_enforced_on_a_bigint_string() {
    let schema = parse(LEDGER);
    let model = schema.models.iter().find(|m| m.name == "Ledger").unwrap();
    let check = |capped: &str| {
        validate_payload(
            model,
            &payload(&[
                ("id", serde_json::json!("l1")),
                ("amountE8", serde_json::json!("1")),
                ("capped", serde_json::json!(capped)),
            ]),
            false,
        )
    };

    assert!(check("0").is_empty());
    assert!(check("100").is_empty());
    for bad in ["-1", "101", "9223372036854775807"] {
        let errors = check(bad);
        assert!(
            errors
                .iter()
                .any(|e| e.field == "capped" && e.code == ValidationCode::Range),
            "{bad} is outside 0..=100 and must fail @range: {errors:?}"
        );
    }
}

#[test]
fn int_fields_still_take_numbers_and_refuse_strings() {
    let schema = parse("model Counter {\n  id String @id\n  hits Int\n}\n");
    let model = schema.models.iter().find(|m| m.name == "Counter").unwrap();
    let check = |hits: serde_json::Value| {
        validate_payload(
            model,
            &payload(&[("id", serde_json::json!("c")), ("hits", hits)]),
            false,
        )
    };
    assert!(check(serde_json::json!(7)).is_empty());
    assert!(
        check(serde_json::json!("7"))
            .iter()
            .any(|e| e.code == ValidationCode::TypeMismatch)
    );
}
