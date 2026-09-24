//! Inputs a legitimate agent may send that serde accepts *and* the schema
//! promises to accept: the alternative scalar spellings `scalar.rs`
//! documents (uppercase `Uuid` hex, lowercase `t`/`z`, numeric offsets)
//! and omitted optionals, down to an empty `PageInput`.
//!
//! `gaps.rs`'s `every_input_the_schema_accepts_deserializes` only checks
//! one direction (schema accepts ⇒ serde accepts), and the round-trip
//! suites only feed what serde *emits* (lowercase hex, `T`/`Z`, every
//! `Option` written as `null`). So a pattern tightened until it rejected
//! these, or an optional field turned required, passed every other test
//! while making a valid call impossible for an agent that follows the
//! schema. Found by mutation in review (cratestack#1037).

use serde_json::{Value, json};

use super::{assert_accepts, tool, values, with};
use crate::cratestack_schema::procedures::{echo_scalars, echo_shapes, page_shapes};

#[test]
fn documented_alternative_scalar_forms_are_accepted() {
    let tool = tool("echoScalars");
    let base = serde_json::to_value(&values::scalars()[1]).unwrap();
    for (field, value) in [
        ("id", json!("0123ABCD-89AB-CDEF-0123-456789ABCDEF")),
        ("id", json!("0123abcd-89AB-cdef-0123-456789AbCdEf")),
        ("at", json!("2024-01-01t00:00:00z")),
        ("at", json!("2024-06-30T12:30:00.5+02:00")),
        ("at", json!("2024-06-30T12:30:00-05:30")),
        ("at", json!("2024-06-30T12:30:00-00:00")),
        ("at", json!("2024-06-30T12:30:00.123456789012Z")),
        ("count", json!(i64::MIN)),
        ("count", json!(i64::MAX)),
    ] {
        let args = json!({ "args": with(&base, field, Some(value.clone())) });
        let context = format!("echoScalars `{field}` = {value}");
        let decoded = serde_json::from_value::<echo_scalars::Args>(args.clone());
        assert!(decoded.is_ok(), "{context}: serde rejects it: {decoded:?}");
        assert_accepts(&tool.input, &args, &context);
    }
}

#[test]
fn omitted_optionals_and_an_empty_page_input_are_accepted() {
    let shapes_tool = tool("echoShapes");
    let mut shapes = serde_json::to_value(&values::shapes()[1]).unwrap();
    let object = shapes.as_object_mut().unwrap();
    for field in [
        "label",
        "maybeStatus",
        "maybeChild",
        "maybeBlob",
        "maybeAmount",
        "maybeAt",
    ] {
        assert!(object.remove(field).is_some(), "fixture lost `{field}`");
    }
    let calls: [(Value, &str); 3] = [
        (
            json!({ "args": shapes, "ids": [] }),
            "no `page`, no `limit`",
        ),
        (
            json!({ "args": shapes, "ids": [], "page": {} }),
            "`page` = {}",
        ),
        (
            json!({ "args": shapes, "ids": [], "page": { "limit": 5 } }),
            "`page` without `offset`",
        ),
    ];
    for (args, context) in calls {
        let decoded = serde_json::from_value::<echo_shapes::Args>(args.clone());
        assert!(decoded.is_ok(), "{context}: serde rejects it: {decoded:?}");
        assert_accepts(&shapes_tool.input, &args, context);
    }

    let page = tool("pageShapes");
    let args = json!({ "page": {} });
    assert!(serde_json::from_value::<page_shapes::Args>(args.clone()).is_ok());
    assert_accepts(&page.input, &args, "pageShapes `page` = {}");
}
