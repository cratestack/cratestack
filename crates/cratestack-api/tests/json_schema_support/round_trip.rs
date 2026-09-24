//! Positive half: serde's output for the real generated `Args` and
//! `Output` types validates against the generated schemas.

use cratestack::{Page, PageInfo, PageInput};
use serde_json::{Value, json};

use super::{assert_accepts, tool, values};
use crate::cratestack_schema::procedures::{
    count_scalars, echo_scalars, echo_shapes, echo_tree, page_shapes,
};

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("generated types serialize")
}

#[test]
fn every_scalar_round_trips_as_input_and_output() {
    let tool = tool("echoScalars");
    let output = tool.output.as_ref().expect("a `type` return is an object");
    for sample in values::scalars() {
        let args = echo_scalars::Args {
            args: sample.clone(),
        };
        assert_accepts(&tool.input, &to_json(&args), "echoScalars input");
        let result: echo_scalars::Output = sample;
        assert_accepts(output, &to_json(&result), "echoScalars output");
    }
}

#[test]
fn optionals_lists_enums_and_nesting_round_trip() {
    let tool = tool("echoShapes");
    let output = tool.output.as_ref().expect("a `type` return is an object");
    let pages = [
        None,
        Some(PageInput::default()),
        Some(PageInput {
            limit: Some(i64::MAX),
            offset: Some(0),
        }),
    ];
    for (i, shapes) in values::shapes().into_iter().enumerate() {
        let args = echo_shapes::Args {
            args: shapes.clone(),
            page: pages[i % pages.len()],
            limit: (i % 2 == 0).then_some(-5),
            ids: shapes.ids.clone(),
        };
        assert_accepts(&tool.input, &to_json(&args), "echoShapes input");
        let result: echo_shapes::Output = shapes;
        assert_accepts(output, &to_json(&result), "echoShapes output");
    }
}

#[test]
fn page_and_page_input_round_trip() {
    let tool = tool("pageShapes");
    let output = tool.output.as_ref().expect("`Page<T>` is an object");
    let info = |limit, offset, next, previous| PageInfo {
        limit,
        offset,
        has_next_page: next,
        has_previous_page: previous,
    };
    let pages: [page_shapes::Output; 3] = [
        Page::new(Vec::new(), PageInfo::default()),
        Page::new(values::shapes(), info(Some(2), Some(0), true, false)).with_total_count(Some(9)),
        Page::new(values::shapes(), info(None, Some(4), false, true)).with_total_count(Some(0)),
    ];
    for page in pages {
        assert_accepts(output, &to_json(&page), "pageShapes output");
    }
    for page in [
        PageInput::default(),
        PageInput {
            limit: Some(1),
            offset: Some(i64::MAX),
        },
    ] {
        let args = page_shapes::Args { page };
        assert_accepts(&tool.input, &to_json(&args), "pageShapes input");
    }
}

#[test]
fn a_self_referencing_type_round_trips() {
    let tool = tool("echoTree");
    let tree = values::tree();
    let args = echo_tree::Args { tree: tree.clone() };
    assert_accepts(&tool.input, &to_json(&args), "echoTree input");
    let result: echo_tree::Output = tree;
    let output = tool.output.as_ref().expect("a `type` return is an object");
    assert_accepts(output, &to_json(&result), "echoTree output");
}

#[test]
fn a_scalar_return_has_no_output_schema() {
    let tool = tool("countScalars");
    assert!(tool.output.is_none(), "`Int` is not an object");
    for filter in [None, Some("x".to_owned())] {
        let args = count_scalars::Args { filter };
        assert_accepts(&tool.input, &to_json(&args), "countScalars input");
    }
    // serde's derive lets a missing `Option` field default to `None`, so
    // the schema must not require it.
    assert!(serde_json::from_value::<count_scalars::Args>(json!({})).is_ok());
    assert_accepts(
        &tool.input,
        &json!({}),
        "countScalars with `filter` omitted",
    );
}

// `a_json_argument_is_refused_through_macro_expansion` used to live here,
// reading the refusal out of phase 2's probe as data. Since phase 3 a
// `Json` tool is a compile error, so the same check is a trybuild case:
// `cratestack-macros`' `tests/ui_mcp.rs`, `mcp_json_tool_refused`.
