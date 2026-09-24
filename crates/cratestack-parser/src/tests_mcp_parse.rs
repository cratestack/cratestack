//! MCP syntax (cratestack#1036): the accepted forms become typed IR, and
//! every malformed form is a parse error rather than an inert attribute.

use crate::parse_schema;
use crate::tests_mcp_support::{VALID, edit, syntax_error};

#[test]
fn the_block_and_both_attributes_parse_into_typed_ir() {
    let schema = parse_schema(VALID).expect("the MCP fixture is valid");

    let mcp = schema.mcp.as_ref().expect("typed mcp block");
    assert_eq!(mcp.docs, vec!["Agent-facing surface.".to_owned()]);
    let (tools, resources) = (mcp.expose_tools.unwrap(), mcp.expose_resources.unwrap());
    assert_eq!(&VALID[tools.start..tools.end], "tools");
    assert_eq!(&VALID[resources.start..resources.end], "resources");
    assert!(VALID[mcp.span.start..mcp.span.end].starts_with("mcp {"));
    assert!(VALID[mcp.span.start..mcp.span.end].ends_with('}'));
    assert!(
        schema.config_blocks.is_empty(),
        "mcp no longer lands in config_blocks as raw text"
    );

    let post = &schema.models[0];
    let resource = post.mcp.as_ref().expect("@@mcp on Post");
    assert_eq!(resource.resource, "posts");
    assert_eq!(resource.max_page_size, Some(20));
    assert_eq!(
        &VALID[resource.span.start..resource.span.end],
        r#"@@mcp(resource: "posts", max_page_size: 20)"#
    );
    assert!(
        post.attributes.iter().all(|a| !a.raw.contains("mcp")),
        "@@mcp is extracted, not left behind as a raw attribute"
    );

    let feed = schema.procedures[0].mcp.as_ref().expect("@mcp on getFeed");
    assert_eq!(feed.tool_name, "getFeed", "Q2: the name as written");
    assert!(feed.tool_name_defaulted);
    assert_eq!(feed.description, None);

    let publish = &schema.procedures[1];
    let tool = publish.mcp.as_ref().expect("@mcp on publishPost");
    assert_eq!(tool.tool_name, "publish_post");
    assert!(!tool.tool_name_defaulted);
    assert_eq!(
        tool.description.as_deref(),
        Some("Publish a draft, then notify: now."),
        "a comma and a colon inside the description do not split it"
    );
    assert!(publish.attributes.iter().all(|a| !a.raw.contains("mcp")));
}

#[test]
fn optional_arguments_may_be_omitted_and_reordered() {
    let schema = parse_schema(&edit(
        r#"@@mcp(resource: "posts", max_page_size: 20)"#,
        r#"@@mcp(resource: "posts")"#,
    ))
    .expect("max_page_size is optional");
    assert_eq!(schema.models[0].mcp.as_ref().unwrap().max_page_size, None);

    let schema = parse_schema(&edit(
        r#"@mcp(tool: "publish_post", description: "Publish a draft, then notify: now.")"#,
        r#"@mcp(description: "d", tool)"#,
    ))
    .expect("argument order is free");
    let tool = schema.procedures[1].mcp.as_ref().unwrap();
    assert_eq!(tool.tool_name, "publishPost");
    assert_eq!(tool.description.as_deref(), Some("d"));
}

#[test]
fn a_schema_without_mcp_serializes_exactly_as_before() {
    let schema = parse_schema("model A {\n  id Int @id\n}\n").expect("parses");
    let json = serde_json::to_string(&schema).expect("serializes");
    assert!(!json.contains("\"mcp\""), "{json}");
}

#[test]
fn malformed_attribute_shapes_are_parse_errors() {
    let tool = r#"@mcp(tool: "publish_post", description: "Publish a draft, then notify: now.")"#;
    let resource = r#"@@mcp(resource: "posts", max_page_size: 20)"#;
    let cases = [
        (tool, "@mcp", "must be written `@mcp(tool)`"),
        (tool, "@mcp()", "must contain `tool`"),
        (tool, r#"@mcp(description: "d")"#, "must contain `tool`"),
        (tool, "@mcp(tool: publish_post)", "must be a string literal"),
        (tool, "@mcp(tool, tool)", "repeats `tool`"),
        (
            tool,
            "@mcp(tool, description: 3)",
            "must be a string literal",
        ),
        (tool, "@mcp(tool, name: \"x\")", "unknown argument `name`"),
        (tool, "@mcp(tool,)", "empty argument"),
        (tool, r#"@mcp(tool: "x)"#, "unterminated string"),
        (tool, r#"@mcp(resource: "x")"#, "belongs on a model"),
        (resource, "@@mcp", "must be written `@@mcp(resource"),
        (
            resource,
            "@@mcp(max_page_size: 5)",
            "must contain `resource",
        ),
        (
            resource,
            "@@mcp(resource: posts)",
            "must be a string literal",
        ),
        (
            resource,
            r#"@@mcp(resource: "a", max_page_size: "5")"#,
            "must be an integer",
        ),
        (
            resource,
            r#"@@mcp(resource: "a", max_page_size: -1)"#,
            "must be an integer",
        ),
        (
            resource,
            r#"@@mcp(resource: "a", resource: "b")"#,
            "repeats `resource`",
        ),
        (
            resource,
            r#"@@mcp(resource: "a", tool)"#,
            "belongs on a procedure",
        ),
    ];
    for (from, to, needle) in cases {
        let message = syntax_error(&edit(from, to));
        assert!(message.contains(needle), "{to}: {message}");
    }
}

#[test]
fn the_dotted_form_is_rejected_with_the_d1_hint() {
    let message = syntax_error(&edit("@mcp(tool)", "@mcp.tool"));
    assert!(message.contains("dotted form"), "{message}");
    assert!(message.contains("ADR 0002 D1"), "{message}");
    let message = syntax_error(&edit(
        r#"@@mcp(resource: "posts", max_page_size: 20)"#,
        r#"@@mcp.resource("posts")"#,
    ));
    assert!(message.contains("ADR 0002 D1"), "{message}");
}

#[test]
fn an_attribute_repeated_on_one_declaration_is_rejected() {
    let message = syntax_error(&edit(
        "  @mcp(tool)\n",
        "  @mcp(tool)\n  @mcp(tool: \"b\")\n",
    ));
    assert!(message.contains("more than once"), "{message}");
    let message = syntax_error(&edit(
        "  @@mcp(resource",
        "  @@mcp(resource: \"x\")\n  @@mcp(resource",
    ));
    assert!(message.contains("more than once"), "{message}");
}

#[test]
fn an_mcp_attribute_sharing_a_line_is_rejected_not_swallowed() {
    let message = syntax_error(&edit(
        "  @allow(auth() != null)\n  @mcp(tool)\n",
        "  @allow(auth() != null) @mcp(tool)\n",
    ));
    assert!(message.contains("must be on its own line"), "{message}");
}
