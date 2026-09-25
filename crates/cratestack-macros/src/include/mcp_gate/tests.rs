//! The gate's decisions in both feature states. The real compile errors
//! (feature off, embedded, `Json`, `@stream`) are pinned by
//! `tests/ui_mcp.rs`; the feature-on side is what the facades' `mcp` test
//! suites compile, and is only reachable here, because this crate's own
//! tests build without the feature.

use super::plan::{mcp_declarations, server_plan};

const TOOLS_ONLY: &str = r#"
datasource db {
  provider = "none"
}

mcp {
  expose = [tools]
}

type Args {
  n Int
}

procedure getFeed(args: Args): Args
  @allow(true)
  @mcp(tool)

mutation procedure publish(args: Args): Boolean
  @allow(true)
  @mcp(tool: "publish_post", description: "Publish.")
"#;

const WITH_RESOURCES: &str = r#"
datasource db {
  provider = "postgresql"
}

mcp {
  name = "journal"
  expose = [tools, resources]
}

type Args {
  n Int
}

model Post {
  id Int @id

  @@allow("read", true)
  @@mcp(resource: "posts")
}

procedure getFeed(args: Args): Post[]
  @allow(true)
  @mcp(tool)
"#;

fn parse(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("valid MCP schema")
}

#[test]
fn names_every_mcp_declaration() {
    assert_eq!(
        mcp_declarations(&parse(WITH_RESOURCES)).as_deref(),
        Some(
            "an `mcp { }` block, `@mcp(tool)` on procedure `getFeed`, \
             `@@mcp(resource: ...)` on model `Post`"
        )
    );
    let plain = parse("model Post {\n  id Int @id\n}\n");
    assert_eq!(mcp_declarations(&plain), None);
}

#[test]
fn feature_on_tools_are_planned_in_declaration_order() {
    let schema = parse(TOOLS_ONLY);
    let plans = server_plan(&schema, None, true)
        .expect("tools are served")
        .tools;
    let names: Vec<&str> = plans.iter().map(|plan| plan.name.as_str()).collect();
    assert_eq!(names, ["getFeed", "publish_post"]);
    assert_eq!(plans[1].description.as_deref(), Some("Publish."));
    assert!(plans[0].input.contains(r#""args""#), "{}", plans[0].input);
    assert!(plans[0].output.is_some(), "`Args` is an object");
    assert!(plans[1].output.is_none(), "`Boolean` is not");
}

#[test]
fn feature_off_asks_for_the_feature() {
    let schema = parse(TOOLS_ONLY);
    let message = server_plan(&schema, None, false).err().expect("gated");
    assert!(
        message.contains("without its `mcp` Cargo feature"),
        "{message}"
    );
    assert!(message.contains(r#"features = ["mcp"]"#), "{message}");
}

/// Phase 5 (cratestack#1040) lifted the refusal this test used to pin
/// (`resources_stay_gated_with_the_feature_on`): with the feature on, a
/// resource is planned, not refused.
#[test]
fn feature_on_resources_are_planned_beside_tools() {
    let schema = parse(WITH_RESOURCES);
    let plan = server_plan(&schema, None, true).expect("served");
    assert_eq!(plan.tools.len(), 1);
    let [post] = plan.resources.as_slice() else {
        panic!("one resource");
    };
    assert_eq!(post.segment, "posts");
    assert_eq!(post.authority, "journal", "the block's `name`");
    assert_eq!(
        post.max_page_size, 200,
        "no `max_page_size:` means Q3's 200"
    );
    assert_eq!(post.primary_key.name, "id");
}

#[test]
fn feature_off_still_refuses_resources() {
    let schema = parse(WITH_RESOURCES);
    let message = server_plan(&schema, None, false).err().expect("gated");
    assert!(
        message.contains("without its `mcp` Cargo feature"),
        "{message}"
    );
    assert!(message.contains("model `Post`"), "{message}");
}

#[test]
fn an_unmappable_tool_is_refused_in_both_feature_states() {
    let schema = parse(&TOOLS_ONLY.replace(
        "procedure getFeed(args: Args): Args",
        "procedure getFeed(payload: Json): Args",
    ));
    for feature in [false, true] {
        let message = server_plan(&schema, None, feature).err().expect("refused");
        assert!(
            message.starts_with("`@mcp(tool)` on procedure `getFeed` cannot be exposed"),
            "{message}"
        );
        assert!(
            message.contains("`Json` has no faithful JSON Schema mapping"),
            "{message}"
        );
    }
}
