//! Negative half: wrong-shaped values are rejected by the schema *and* by
//! serde. Asserting serde's rejection too proves each case really is
//! wrong on the wire, rather than merely disliked by the schema. A
//! permissive `{}` schema fails every test here.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{Tool, assert_rejects, tool, values, with};
use crate::cratestack_schema::procedures::{echo_scalars, echo_shapes, echo_tree, page_shapes};
use crate::cratestack_schema::{Scalars, Shapes, Tree};

fn parse(literal: &str) -> Value {
    serde_json::from_str(literal).unwrap_or_else(|e| panic!("bad JSON literal {literal}: {e}"))
}

/// Rejected by the input schema (as argument `arg`) and by the
/// output schema, and by serde as both `A` and `O`.
fn reject_both<A: DeserializeOwned, O: DeserializeOwned>(
    tool: &Tool,
    arg: &str,
    value: &Value,
    context: &str,
) {
    let args = Value::Object([(arg.to_owned(), value.clone())].into_iter().collect());
    assert_rejects(&tool.input, &args, context);
    assert!(
        serde_json::from_value::<A>(args).is_err(),
        "{context}: serde accepted it"
    );
    if let Some(output) = &tool.output {
        assert_rejects(output, value, context);
        let accepted = serde_json::from_value::<O>(value.clone()).is_ok();
        assert!(!accepted, "{context}: serde accepted it as output");
    }
}

pub fn wrong_scalars() -> Vec<(&'static str, Value)> {
    let mut cases = vec![
        ("text", json!(1)),
        ("text", json!(null)),
        ("text", json!(["x"])),
        ("cuid", json!(true)),
        ("count", json!("1")),
        ("count", json!(1.5)),
        ("count", json!(null)),
        ("count", json!(9_223_372_036_854_775_808_u64)),
        ("count", json!(u64::MAX)),
        ("ratio", json!("1.5")),
        ("ratio", json!(null)),
        ("flag", json!("true")),
        ("flag", json!(1)),
        ("at", json!("yesterday")),
        ("at", json!(1_700_000_000)),
        ("at", json!("2024-01-01T00:00:00")),
        ("at", json!("2024-01-01")),
        ("blob", json!("AAE=")),
        ("blob", json!([256])),
        ("blob", json!([-1])),
        ("blob", json!([1.5])),
        ("blob", json!(null)),
        ("id", json!("not-a-uuid")),
        ("id", json!("00000000-0000-0000-0000-00000000123")),
        ("id", json!(7)),
        ("status", json!("archived")),
        ("status", json!("Deleted")),
        ("status", json!(0)),
        ("amount", json!(null)),
        ("amount", json!("abc")),
        ("amount", json!("1.5.5")),
        ("amount", json!(" 1.5")),
    ];
    cases.extend(
        crate::DECIMAL
            .wrong
            .iter()
            .map(|literal| ("amount", parse(literal))),
    );
    cases
}

#[test]
fn each_scalar_rejects_wrong_shapes() {
    let tool = tool("echoScalars");
    let base = serde_json::to_value(&values::scalars()[1]).unwrap();
    for (field, value) in wrong_scalars() {
        let context = format!("echoScalars `{field}` = {value}");
        let instance = with(&base, field, Some(value));
        reject_both::<echo_scalars::Args, Scalars>(&tool, "args", &instance, &context);
    }
}

#[test]
fn missing_required_fields_are_rejected() {
    let tool = tool("echoScalars");
    let base = serde_json::to_value(&values::scalars()[1]).unwrap();
    for field in base.as_object().unwrap().keys() {
        let instance = with(&base, field, None);
        let context = format!("echoScalars without `{field}`");
        reject_both::<echo_scalars::Args, Scalars>(&tool, "args", &instance, &context);
    }
    let (shapes_tool, shapes) = (tool_shapes(), tool_shapes_base());
    for field in ["tags", "statuses", "child", "children", "blobs", "ids"] {
        let instance = with(&shapes, field, None);
        let context = format!("echoShapes without `{field}`");
        reject_both::<echo_shapes::Args, Shapes>(&shapes_tool, "args", &instance, &context);
    }
}

fn tool_shapes() -> Tool {
    tool("echoShapes")
}

fn tool_shapes_base() -> Value {
    serde_json::to_value(&values::shapes()[1]).unwrap()
}

#[test]
fn optionals_lists_enums_and_nesting_reject_wrong_shapes() {
    let tool = tool_shapes();
    let base = tool_shapes_base();
    for (path, value) in [
        ("label", json!(5)),
        ("tags", json!("a")),
        ("tags", json!([1])),
        ("tags", json!(null)),
        ("statuses", json!(["Gone"])),
        ("maybeStatus", json!("nope")),
        ("child", json!(null)),
        ("child.count", json!("1")),
        ("children", json!({})),
        ("children", json!([{}])),
        ("maybeChild", json!("x")),
        ("blobs", json!([[256]])),
        ("blobs", json!(["AAE="])),
        ("maybeBlob", json!("AAE=")),
        ("maybeAmount", json!("abc")),
        ("maybeAt", json!("yesterday")),
        ("ids", json!(["nope"])),
    ] {
        let context = format!("echoShapes `{path}` = {value}");
        let instance = with(&base, path, Some(value));
        reject_both::<echo_shapes::Args, Shapes>(&tool, "args", &instance, &context);
    }
}

#[test]
fn arguments_reject_wrong_shapes() {
    let tool = tool_shapes();
    let args = json!({ "args": tool_shapes_base(), "ids": [] });
    for (path, value) in [
        ("args", None),
        ("ids", None),
        ("ids", Some(json!("x"))),
        ("page", Some(json!({ "limit": "10" }))),
        ("page", Some(json!("x"))),
        ("limit", Some(json!(1.5))),
    ] {
        let instance = with(&args, path, value.clone());
        let context = format!("echoShapes argument `{path}` = {value:?}");
        assert_rejects(&tool.input, &instance, &context);
        assert!(
            serde_json::from_value::<echo_shapes::Args>(instance).is_err(),
            "{context}"
        );
    }
}

#[test]
fn page_and_tree_outputs_reject_wrong_shapes() {
    let tool = tool("pageShapes");
    let output = tool.output.as_ref().unwrap();
    let page: page_shapes::Output = cratestack::Page::new(values::shapes(), Default::default());
    let base = serde_json::to_value(&page).unwrap();
    for (path, value) in [
        ("items", None),
        ("pageInfo", None),
        ("pageInfo.hasNextPage", None),
        ("pageInfo.limit", Some(json!("10"))),
        ("totalCount", Some(json!("1"))),
        ("items", Some(json!([{}]))),
    ] {
        let instance = with(&base, path, value.clone());
        let context = format!("pageShapes output `{path}` = {value:?}");
        assert_rejects(output, &instance, &context);
        assert!(
            serde_json::from_value::<page_shapes::Output>(instance).is_err(),
            "{context}"
        );
    }
    let tree = super::tool("echoTree");
    let bad = json!({ "label": "root", "children": [{ "label": 1, "children": [] }] });
    reject_both::<echo_tree::Args, Tree>(&tree, "tree", &bad, "echoTree nested label");
}
