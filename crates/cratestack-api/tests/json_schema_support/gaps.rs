//! Where the schema and serde deliberately or unavoidably disagree. Each
//! disagreement is asserted exactly, so a change on either side (a serde
//! upgrade, a looser pattern) flips a test instead of drifting silently.
//! The list is mirrored in `cratestack-macros/src/json_schema.rs`'s
//! module doc.

use serde_json::{Value, json};

use super::negative::wrong_scalars;
use super::{assert_rejects, tool, values, with};
use crate::cratestack_schema::Scalars;
use crate::cratestack_schema::procedures::echo_scalars;

fn scalars_base() -> Value {
    serde_json::to_value(&values::scalars()[1]).unwrap()
}

fn serde_accepts(instance: &Value) -> bool {
    serde_json::from_value::<Scalars>(instance.clone()).is_ok()
}

fn parse(literal: &str) -> Value {
    serde_json::from_str(literal).unwrap_or_else(|e| panic!("bad JSON literal {literal}: {e}"))
}

/// Inputs serde takes that the schema refuses, because the schema
/// describes what serde *emits*. An agent following the schema is never
/// hurt by these.
fn stricter() -> Vec<(&'static str, Value)> {
    let mut cases = vec![
        ("id", json!("00000000000000000000000000001234")),
        ("id", json!("{00000000-0000-0000-0000-000000001234}")),
        ("id", json!("urn:uuid:00000000-0000-0000-0000-000000001234")),
        ("at", json!("2024-01-01 00:00:00Z")),
    ];
    cases.extend(
        crate::DECIMAL
            .stricter
            .iter()
            .map(|literal| ("amount", parse(literal))),
    );
    cases
}

/// Inputs the schema takes that serde refuses. JSON Schema can't express
/// these distinctions; see the module doc of `json_schema.rs`.
fn known_gaps() -> Vec<(&'static str, Value)> {
    let mut cases = vec![
        // `integer` means "zero fractional part", so `1.0` qualifies.
        ("count", json!(1.0)),
        // The pattern checks RFC 3339's shape, not the calendar.
        ("at", json!("2024-02-30T00:00:00Z")),
        ("at", json!("2024-01-01T25:00:00Z")),
    ];
    cases.extend(
        crate::DECIMAL
            .gaps
            .iter()
            .map(|literal| ("amount", parse(literal))),
    );
    cases
}

#[test]
fn the_schema_is_stricter_than_serde_only_where_listed() {
    let tool = tool("echoScalars");
    let base = scalars_base();
    for (field, value) in stricter() {
        let instance = with(&base, field, Some(value.clone()));
        let context = format!("`{field}` = {value}");
        assert!(
            serde_accepts(&instance),
            "{context}: serde no longer accepts it"
        );
        assert_rejects(&tool.input, &json!({ "args": instance }), &context);
    }
}

#[test]
fn the_schema_is_looser_than_serde_only_where_listed() {
    let tool = tool("echoScalars");
    let base = scalars_base();
    for (field, value) in known_gaps() {
        let instance = with(&base, field, Some(value.clone()));
        let context = format!("`{field}` = {value}");
        assert!(
            !serde_accepts(&instance),
            "{context}: serde now accepts it; drop the gap"
        );
        let args = json!({ "args": instance });
        assert!(
            tool.input.is_valid(&args),
            "{context}: the schema now rejects it; drop the gap"
        );
    }
}

/// The direction an agent depends on: whatever the schema lets through,
/// serde deserializes. Probed with every wrong, stricter and alternative
/// form the other tests use, plus valid variants serde doesn't emit.
#[test]
fn every_input_the_schema_accepts_deserializes() {
    let tool = tool("echoScalars");
    let base = scalars_base();
    let gaps = known_gaps();
    let mut candidates = wrong_scalars();
    candidates.extend(stricter());
    candidates.extend([
        ("at", json!("2024-01-01t00:00:00z")),
        ("at", json!("2024-06-30T12:30:00.5+02:00")),
        ("at", json!("2016-12-31T23:59:60Z")),
        ("id", json!("00000000-0000-0000-0000-00000000ABCD")),
        ("count", json!(-0)),
        ("ratio", json!(7)),
        ("ratio", json!(-1e-310)),
        ("blob", json!([])),
        ("text", json!("")),
        ("amount", json!("-0")),
        ("amount", json!("0.5")),
    ]);
    candidates.extend(crate::DECIMAL.samples.iter().map(|s| ("amount", json!(s))));
    let mut accepted = 0;
    for (field, value) in candidates {
        if gaps.contains(&(field, value.clone())) {
            continue;
        }
        let instance = with(&base, field, Some(value.clone()));
        let args = json!({ "args": instance });
        if tool.input.is_valid(&args) {
            accepted += 1;
            let decoded = serde_json::from_value::<echo_scalars::Args>(args);
            assert!(
                decoded.is_ok(),
                "`{field}` = {value}: schema accepts, serde rejects: {decoded:?}"
            );
        }
    }
    assert!(
        accepted >= 10,
        "only {accepted} candidates were schema-valid; the probe is too weak"
    );
}

/// Outputs serde can write that the schema rejects. serde_json writes a
/// non-finite `f64` as `null`, which no `number` schema allows, and a
/// `null`-tolerant one would be wrong for input, where serde rejects
/// `null`. chrono writes a year past 9999 as `+10000-…`, which is not RFC
/// 3339, so neither the pattern nor `format: date-time` allows it.
#[test]
fn outputs_outside_the_schema_are_only_the_listed_ones() {
    let tool = tool("echoScalars");
    let output = tool.output.as_ref().unwrap();
    for ratio in [f64::NAN, f64::INFINITY] {
        let mut sample = values::scalars()[0].clone();
        sample.ratio = ratio;
        let written = serde_json::to_value(&sample).unwrap();
        assert_eq!(written["ratio"], Value::Null);
        assert_rejects(output, &written, "non-finite `ratio`");
    }
    let mut sample = values::scalars()[0].clone();
    sample.at = cratestack::chrono::DateTime::from_timestamp(253_402_300_800, 0).unwrap();
    let written = serde_json::to_value(&sample).unwrap();
    assert_eq!(written["at"], "+10000-01-01T00:00:00Z");
    assert_rejects(output, &written, "`at` in year 10000");
}
