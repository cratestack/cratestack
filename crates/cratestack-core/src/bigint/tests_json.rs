//! `BigInt` through `serde_json`: a string on the way out, a canonical string
//! and nothing else on the way back.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::BigInt;
use super::tests::{BOUNDARIES, REFUSED_STRINGS};

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

#[test]
fn serializes_as_a_json_string_never_a_number() {
    for (value, text) in BOUNDARIES {
        assert_eq!(
            serde_json::to_value(BigInt::new(value)).unwrap(),
            json!(text)
        );
    }
}

#[test]
fn round_trips_through_serde_json_in_a_struct() {
    for (value, text) in BOUNDARIES {
        let encoded = serde_json::to_string(&row(value)).unwrap();
        assert_eq!(
            encoded,
            format!(r#"{{"amountE8":"{text}","maybe":"{text}","list":["{text}"]}}"#)
        );
        assert_eq!(serde_json::from_str::<Row>(&encoded).unwrap(), row(value));
    }
}

#[test]
fn round_trips_through_serde_json_value_the_batch_path() {
    for (value, text) in BOUNDARIES {
        let frame: Value = serde_json::to_value(row(value)).unwrap();
        assert_eq!(frame["amountE8"], json!(text));
        assert_eq!(serde_json::from_value::<Row>(frame).unwrap(), row(value));
    }
}

#[test]
fn json_refuses_a_number_and_every_other_item() {
    for item in [
        json!(9_007_199_254_740_993_u64),
        json!(-5),
        json!(0),
        json!(1.0),
        json!(1.5),
        json!(true),
        json!(null),
        json!([]),
        json!({}),
    ] {
        let error = serde_json::from_value::<BigInt>(item.clone()).unwrap_err();
        assert!(
            error.to_string().contains("canonical decimal string"),
            "{item} must be refused with the expected-form message, got: {error}"
        );
    }
}

#[test]
fn json_refuses_every_non_canonical_string() {
    for text in REFUSED_STRINGS {
        let error = serde_json::from_value::<BigInt>(json!(text)).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("canonical") || message.contains("64-bit"),
            "{message}"
        );
    }
}

#[test]
fn a_refused_string_is_quoted_in_the_error_but_bounded() {
    let message = serde_json::from_value::<BigInt>(json!("+5"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("`+5`"), "{message}");

    let hostile = "9".repeat(10_000);
    let message = serde_json::from_value::<BigInt>(json!(hostile))
        .unwrap_err()
        .to_string();
    assert!(
        message.len() < 300,
        "error repeated the whole input: {} bytes",
        message.len()
    );
    assert!(message.contains("..."), "{message}");
}

#[derive(Debug, Deserialize)]
struct Flattened {
    #[serde(flatten)]
    inner: Row,
}

#[test]
fn a_buffered_flatten_path_refuses_a_number_too() {
    // `flatten` buffers into serde's `Content` first, so this reaches the
    // visitor through a different deserializer than `serde_json`'s own.
    let ok = r#"{"amountE8":"1","maybe":null,"list":[]}"#;
    assert_eq!(
        serde_json::from_str::<Flattened>(ok)
            .unwrap()
            .inner
            .amount_e8,
        BigInt::new(1)
    );
    for body in [
        r#"{"amountE8":1,"maybe":null,"list":[]}"#,
        r#"{"amountE8":"1","maybe":null,"list":[9007199254740993]}"#,
    ] {
        assert!(serde_json::from_str::<Flattened>(body).is_err(), "{body}");
    }
}
