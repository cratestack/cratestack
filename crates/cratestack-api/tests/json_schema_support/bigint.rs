//! ADR 0019: `BigInt` against the generated schemas, in every position the
//! fixture reaches it: a bare procedure argument (`echoBig`), a field of
//! `Scalars` and `Shapes`, and each arity.
//!
//! The contract these pin: the schema is a string with the canonical
//! decimal pattern and never an `integer`; `i64::MAX`, `i64::MIN` and
//! `2^53 + 1` validate as strings; a JSON number is refused by the schema
//! and by serde alike. The one place the schema is looser than serde, the
//! `i64` bound, is pinned in `out_of_range_values_are_the_one_documented_gap`
//! and mirrored in `cratestack-macros/src/json_schema.rs`'s module doc.

mod grammar;
mod refused;

use cratestack::BigInt;
use serde_json::{Value, json};

use super::values::{BIG_INTS, scalars};
use super::{Tool, assert_accepts, assert_rejects, tool};
use crate::cratestack_schema::procedures::echo_big;

/// `BigInt`'s fragment spelled out, so a stray `format`, `minimum` or a
/// different pattern fails here by name.
fn fragment() -> Value {
    json!({ "type": "string", "pattern": "^(0|-?[1-9][0-9]{0,18})$" })
}

/// ADR 0019's three pinned values, as the text the wire carries.
const PINNED: [(&str, i64); 3] = [
    ("9223372036854775807", i64::MAX),
    ("-9223372036854775808", i64::MIN),
    ("9007199254740993", 9_007_199_254_740_993),
];

fn descriptor_schema(name: &str) -> Value {
    let descriptor = crate::cratestack_schema::mcp::TOOLS
        .iter()
        .find(|descriptor| descriptor.name == name)
        .unwrap_or_else(|| panic!("no tool `{name}` in the fixture"));
    serde_json::from_str(descriptor.input_schema).expect("generated schema is JSON")
}

fn nullable() -> Value {
    json!({ "anyOf": [fragment(), { "type": "null" }] })
}

/// `echoBig`'s arguments with `total`, `maybe` (when `Some`) and `totals`.
fn echo_big_args(total: Value, maybe: Option<Value>, totals: Value) -> Value {
    let mut args = json!({ "total": total, "totals": totals });
    if let Some(maybe) = maybe {
        args["maybe"] = maybe;
    }
    args
}

/// Refused by the schema and by serde, with `what` naming the case.
fn assert_big_args_refused(tool: &Tool, args: Value, what: &str) {
    assert_rejects(&tool.input, &args, what);
    let decoded = serde_json::from_value::<echo_big::Args>(args);
    assert!(decoded.is_err(), "{what}: serde accepted it");
}

#[test]
fn the_emitted_fragment_is_the_canonical_decimal_string() {
    let echo_big = descriptor_schema("echoBig");
    let properties = &echo_big["properties"];
    assert_eq!(properties["total"], fragment());
    assert_eq!(properties["maybe"], nullable());
    assert_eq!(
        properties["totals"],
        json!({ "type": "array", "items": fragment() })
    );
    assert_eq!(echo_big["required"], json!(["total", "totals"]));
    assert!(
        !echo_big.to_string().contains("integer"),
        "a `BigInt` is never an `integer`: {echo_big}"
    );
    assert!(
        tool("echoBig").output.is_none(),
        "a scalar return has no output schema"
    );

    let scalars = &descriptor_schema("echoScalars")["$defs"]["Scalars"];
    assert_eq!(scalars["properties"]["big"], fragment());
    assert!(
        scalars["required"]
            .as_array()
            .unwrap()
            .contains(&json!("big"))
    );
    let shapes = &descriptor_schema("echoShapes")["$defs"]["Shapes"];
    assert_eq!(shapes["properties"]["maybeBig"], nullable());
    assert_eq!(
        shapes["properties"]["bigs"],
        json!({ "type": "array", "items": fragment() })
    );
    let required = shapes["required"].as_array().unwrap();
    assert!(required.contains(&json!("bigs")));
    assert!(!required.contains(&json!("maybeBig")));
}

#[test]
fn the_pinned_values_validate_as_strings_and_serde_writes_the_same_text() {
    let tool = tool("echoBig");
    for (text, value) in PINNED {
        let wire = echo_big_args(json!(text), Some(json!(text)), json!([text]));
        assert_accepts(&tool.input, &wire, text);
        let decoded: echo_big::Args = serde_json::from_value(wire.clone()).expect("serde decodes");
        assert_eq!(decoded.total, BigInt::new(value), "{text}");
        assert_eq!(decoded.maybe, Some(BigInt::new(value)), "{text}");
        assert_eq!(decoded.totals, [BigInt::new(value)], "{text}");
        assert_eq!(serde_json::to_value(&decoded).unwrap(), wire, "{text}");
    }
}

#[test]
fn every_boundary_value_round_trips_through_the_generated_types() {
    let tool = tool("echoBig");
    let scalars_tool = super::tool("echoScalars");
    for value in BIG_INTS {
        let big = BigInt::new(value);
        let args = echo_big::Args {
            total: big,
            maybe: Some(big),
            totals: BIG_INTS.map(BigInt::new).to_vec(),
        };
        let written = serde_json::to_value(&args).unwrap();
        assert_eq!(written["total"], json!(value.to_string()));
        assert_accepts(&tool.input, &written, "echoBig input");
        let back: echo_big::Args = serde_json::from_value(written).unwrap();
        assert_eq!(back.total, big);
    }
    for sample in scalars() {
        let written = serde_json::to_value(&sample).unwrap();
        assert_eq!(written["big"], json!(sample.big.get().to_string()));
        assert_accepts(
            &scalars_tool.input,
            &json!({ "args": written }),
            "Scalars.big",
        );
    }
}
