//! `BigInt` through the real `JsonCodec` (ADR 0019 D2): a JSON string on the
//! way out, a canonical JSON string and nothing else on the way in, and a
//! decode error whose `detail()` names the field.

use cratestack_codec_json::JsonCodec;
use cratestack_core::{BigInt, CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The five values the contract is pinned at, with their canonical text.
const BOUNDARIES: [(i64, &str); 5] = [
    (i64::MAX, "9223372036854775807"),
    (i64::MIN, "-9223372036854775808"),
    (9_007_199_254_740_993, "9007199254740993"),
    (0, "0"),
    (-1, "-1"),
];

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    amount_e8: BigInt,
    maybe: Option<BigInt>,
    list: Vec<BigInt>,
}

fn row(value: i64) -> Row {
    Row {
        amount_e8: BigInt::new(value),
        maybe: Some(BigInt::new(value)),
        list: vec![BigInt::new(value)],
    }
}

/// The error's `detail()`, which is where the field path lives.
fn detail_of(result: Result<Row, CratestackError>) -> String {
    let error = result.expect_err("the body must be refused");
    assert!(matches!(error, CratestackError::Codec(_)), "{error:?}");
    assert_eq!(error.public_message(), "invalid request payload");
    error
        .detail()
        .expect("a codec error has a detail")
        .to_owned()
}

fn decode_row(body: &str) -> Result<Row, CratestackError> {
    JsonCodec.decode::<Row>(body.as_bytes())
}

#[test]
fn boundary_values_are_json_strings_and_round_trip() {
    for (value, text) in BOUNDARIES {
        let bytes = JsonCodec.encode(&row(value)).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            format!(r#"{{"amountE8":"{text}","maybe":"{text}","list":["{text}"]}}"#)
        );
        assert_eq!(JsonCodec.decode::<Row>(&bytes).unwrap(), row(value));
    }
}

#[test]
fn boundary_values_round_trip_through_serde_json_value_the_batch_path() {
    for (value, text) in BOUNDARIES {
        let frame: Value = serde_json::to_value(row(value)).unwrap();
        assert_eq!(frame["amountE8"], json!(text));
        let bytes = JsonCodec.encode(&frame).unwrap();
        assert_eq!(JsonCodec.decode::<Row>(&bytes).unwrap(), row(value));
        assert_eq!(JsonCodec.decode::<Value>(&bytes).unwrap(), frame);
    }
}

#[test]
fn a_json_number_is_refused_and_the_field_is_named() {
    for number in [
        "9007199254740993",
        "9223372036854775807",
        "-9223372036854775808",
        "0",
        "-1",
        "1.0",
        "1e3",
    ] {
        let detail = detail_of(decode_row(&format!(
            r#"{{"amountE8":{number},"maybe":null,"list":[]}}"#
        )));
        assert!(
            detail.starts_with("failed to decode JSON body: amountE8: "),
            "number {number}: {detail}"
        );
        assert!(detail.contains("canonical decimal string"), "{detail}");
    }
}

#[test]
fn every_non_canonical_string_is_refused_and_the_field_is_named() {
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
        let body = format!(
            r#"{{"amountE8":{},"maybe":null,"list":[]}}"#,
            serde_json::to_string(text).unwrap()
        );
        let detail = detail_of(decode_row(&body));
        assert!(
            detail.starts_with("failed to decode JSON body: amountE8: "),
            "{text:?}: {detail}"
        );
    }
}

#[test]
fn other_json_items_are_refused_and_the_field_is_named() {
    for item in ["true", "null", "[]", "{}", r#"["1"]"#] {
        let detail = detail_of(decode_row(&format!(
            r#"{{"amountE8":{item},"maybe":null,"list":[]}}"#
        )));
        assert!(detail.contains("amountE8: "), "{item}: {detail}");
    }
}

#[test]
fn the_path_reaches_optional_and_list_fields() {
    let detail = detail_of(decode_row(r#"{"amountE8":"1","maybe":5,"list":[]}"#));
    assert!(detail.contains("maybe: "), "{detail}");

    let detail = detail_of(decode_row(
        r#"{"amountE8":"1","maybe":null,"list":["1",2]}"#,
    ));
    assert!(detail.contains("list[1]: "), "{detail}");

    let detail = detail_of(decode_row(
        r#"{"amountE8":"1","maybe":null,"list":["1","+2"]}"#,
    ));
    assert!(detail.contains("list[1]: "), "{detail}");
}

#[test]
fn a_bare_big_int_has_no_path_to_name() {
    let error = JsonCodec.decode::<BigInt>(b"5").unwrap_err();
    let detail = error.detail().unwrap();
    assert!(
        detail.starts_with("failed to decode JSON body: invalid type"),
        "{detail}"
    );
    assert_eq!(
        JsonCodec.decode::<BigInt>(b"\"5\"").unwrap(),
        BigInt::new(5)
    );
}

#[test]
fn trailing_data_is_still_refused_and_trailing_whitespace_is_not() {
    let body = r#"{"amountE8":"1","maybe":null,"list":[]}"#;
    assert!(decode_row(&format!("{body} \n")).is_ok());
    let detail = detail_of(decode_row(&format!("{body} x")));
    assert!(detail.contains("trailing characters"), "{detail}");
}

#[test]
fn errors_unrelated_to_big_int_keep_their_shape() {
    let detail = detail_of(decode_row(r#"{"maybe":null,"list":[]}"#));
    assert!(detail.contains("missing field `amountE8`"), "{detail}");
}
