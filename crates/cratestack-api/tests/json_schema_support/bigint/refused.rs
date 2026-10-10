//! The refusal half of `bigint.rs`: a JSON number at every position, and the
//! optional and list forms that must follow serde exactly.

use serde_json::json;

use super::super::values::{scalars, shapes};
use super::super::{assert_accepts, assert_rejects, tool, with};
use super::{assert_big_args_refused, echo_big, echo_big_args};
use crate::cratestack_schema::{Scalars, Shapes};

#[test]
fn a_json_number_is_refused_by_the_schema_and_by_serde() {
    let tool = tool("echoBig");
    let numbers = [
        json!(i64::MAX),
        json!(i64::MIN),
        json!(9_007_199_254_740_993_i64),
        json!(0),
        json!(7),
        json!(-1),
        json!(1.0),
        json!(1.5),
        json!(1e19),
        json!(true),
        json!([1]),
        json!({}),
    ];
    for number in numbers {
        let what = format!("number {number}");
        let ok = json!("1");
        assert_big_args_refused(
            &tool,
            echo_big_args(number.clone(), None, json!([])),
            &format!("`total` = {what}"),
        );
        assert_big_args_refused(
            &tool,
            echo_big_args(ok.clone(), Some(number.clone()), json!([])),
            &format!("`maybe` = {what}"),
        );
        assert_big_args_refused(
            &tool,
            echo_big_args(ok, None, json!([number.clone()])),
            &format!("`totals[0]` = {what}"),
        );
    }
    assert_big_args_refused(
        &tool,
        echo_big_args(json!(null), None, json!([])),
        "`total` = null",
    );
    assert_big_args_refused(
        &tool,
        echo_big_args(json!("1"), None, json!([null])),
        "`totals[0]` = null",
    );
}

#[test]
fn a_number_in_a_field_is_refused_on_input_and_output() {
    let scalars_tool = super::tool("echoScalars");
    let output = scalars_tool.output.as_ref().unwrap();
    let base = serde_json::to_value(&scalars()[1]).unwrap();
    for number in [json!(i64::MAX), json!(9_007_199_254_740_993_i64), json!(0)] {
        let instance = with(&base, "big", Some(number.clone()));
        let context = format!("Scalars.big = {number}");
        assert_rejects(&scalars_tool.input, &json!({ "args": instance }), &context);
        assert_rejects(output, &instance, &context);
        assert!(
            serde_json::from_value::<Scalars>(instance).is_err(),
            "{context}"
        );
    }
    // The same for `Shapes`' optional and list forms.
    let shapes_tool = super::tool("echoShapes");
    let base = serde_json::to_value(&shapes()[1]).unwrap();
    for (path, value) in [
        ("maybeBig", json!(7)),
        ("bigs", json!([9_007_199_254_740_993_i64])),
        ("bigs", json!(["1", 2])),
        ("bigs", json!("1")),
        ("bigs", json!(null)),
    ] {
        let instance = with(&base, path, Some(value.clone()));
        let context = format!("Shapes.{path} = {value}");
        let args = json!({ "args": instance, "ids": [] });
        assert_rejects(&shapes_tool.input, &args, &context);
        assert!(
            serde_json::from_value::<Shapes>(instance).is_err(),
            "{context}"
        );
    }
}

#[test]
fn optional_and_list_forms_follow_serde() {
    let tool = tool("echoBig");
    let max = json!("9223372036854775807");
    let accepted = [
        (
            "`maybe` omitted",
            echo_big_args(max.clone(), None, json!([])),
        ),
        (
            "`maybe` null",
            echo_big_args(max.clone(), Some(json!(null)), json!([])),
        ),
        (
            "`totals` of the extremes",
            echo_big_args(
                json!("0"),
                Some(json!("-1")),
                json!(["-9223372036854775808", "9223372036854775807"]),
            ),
        ),
    ];
    for (what, args) in accepted {
        assert_accepts(&tool.input, &args, what);
        let decoded = serde_json::from_value::<echo_big::Args>(args);
        assert!(decoded.is_ok(), "{what}: serde rejects it: {decoded:?}");
    }
    for field in ["total", "totals"] {
        let mut args = echo_big_args(max.clone(), None, json!([]));
        args.as_object_mut().unwrap().remove(field);
        assert_big_args_refused(&tool, args, &format!("`{field}` omitted"));
    }

    let shapes_tool = super::tool("echoShapes");
    let mut base = serde_json::to_value(&shapes()[1]).unwrap();
    let object = base.as_object_mut().unwrap();
    assert!(
        object.remove("maybeBig").is_some(),
        "fixture lost `maybeBig`"
    );
    let args = json!({ "args": base, "ids": [] });
    assert_accepts(&shapes_tool.input, &args, "Shapes without `maybeBig`");
    assert!(serde_json::from_value::<Shapes>(base.clone()).is_ok());
    let missing = with(&base, "bigs", None);
    assert_rejects(
        &shapes_tool.input,
        &json!({ "args": missing, "ids": [] }),
        "Shapes without `bigs`",
    );
    assert!(serde_json::from_value::<Shapes>(missing).is_err());
}
