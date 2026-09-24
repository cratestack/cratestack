//! ADR 0002 § Validation, one test per rule (cratestack#1036). Each test's
//! schema is `tests_mcp_support::VALID` with one edit, and each asserts the rule's own
//! message *and* where its span points — see `tests_mcp_support` for why. The
//! scope rules (block, `expose` list, `db = None`) are in `tests_mcp_scope`.

use crate::tests_mcp_support::{edit, rejected};

const TOOL: &str =
    r#"@mcp(tool: "publish_post", description: "Publish a draft, then notify: now.")"#;
const RESOURCE: &str = r#"@@mcp(resource: "posts", max_page_size: 20)"#;

#[test]
fn rule_malformed_given_tool_name() {
    for bad in ["publish post", "publish/post", ""] {
        let source = edit(TOOL, &format!("@mcp(tool: \"{bad}\")"));
        let span = rejected(&source, "a tool name must match `[A-Za-z0-9_.-]{1,128}`");
        assert!(span.starts_with("@mcp(tool: "), "{bad:?}: {span}");
    }
    let long = "a".repeat(129);
    let source = edit(TOOL, &format!("@mcp(tool: \"{long}\")"));
    rejected(&source, "a tool name must match");
    // The boundary itself is accepted.
    let at_limit = format!("{}ab", "a.b-c_".repeat(21));
    assert_eq!(at_limit.len(), 128);
    let ok = edit(TOOL, &format!("@mcp(tool: \"{at_limit}\")"));
    assert!(
        crate::parse_schema(&ok).is_ok(),
        "128 chars of the allowed set parse"
    );
}

#[test]
fn rule_malformed_defaulted_tool_name() {
    // Procedure names are unbounded, so the Q2 default can break the spec's
    // 128-character limit even though the author typed no tool name at all.
    let long = format!("get{}", "X".repeat(126));
    let source = edit("procedure getFeed(", &format!("procedure {long}("));
    let message = rejected(&source, "defaulted from the procedure name");
    assert_eq!(message, "@mcp(tool)");
}

#[test]
fn rule_malformed_resource_segment() {
    for bad in ["Posts", "blog_posts", "posts/1", ""] {
        let source = edit(RESOURCE, &format!("@@mcp(resource: \"{bad}\")"));
        let span = rejected(&source, "a segment must match `[a-z0-9-]+`");
        assert!(span.starts_with("@@mcp("), "{bad:?}: {span}");
    }
    let ok = edit(RESOURCE, "@@mcp(resource: \"blog-posts-2\")");
    assert!(crate::parse_schema(&ok).is_ok());
}

#[test]
fn rule_max_page_size_out_of_range() {
    for bad in [0, 201, 5000] {
        let source = edit(
            RESOURCE,
            &format!("@@mcp(resource: \"posts\", max_page_size: {bad})"),
        );
        let span = rejected(&source, "is out of range");
        assert!(span.starts_with("@@mcp("), "{bad}: {span}");
    }
    for ok in [1, 200] {
        let source = edit(
            RESOURCE,
            &format!("@@mcp(resource: \"posts\", max_page_size: {ok})"),
        );
        assert!(crate::parse_schema(&source).is_ok(), "{ok} is in range");
    }
}

#[test]
fn rule_duplicate_tool_name() {
    // `getFeed`'s defaulted name collides with an explicit one.
    let source = edit(TOOL, "@mcp(tool: \"getFeed\")");
    let span = rejected(&source, "duplicate MCP tool name `getFeed`");
    assert_eq!(span, "@mcp(tool: \"getFeed\")");
}

#[test]
fn rule_duplicate_resource_segment() {
    let source = edit(
        "procedure getFeed",
        "model Draft {\n  id Int @id\n\n  @@allow(\"all\", true)\n  \
         @@mcp(resource: \"posts\")\n}\n\nprocedure getFeed",
    );
    let span = rejected(&source, "duplicate MCP resource segment `posts`");
    assert_eq!(span, "@@mcp(resource: \"posts\")");
}

#[test]
fn rule_resource_needs_a_read_allow() {
    let needle = "exposes a model with no read allow";
    for policies in [
        "",
        "@@allow(\"create\", true)",
        "@@allow(\"list\", true)",
        "@@deny(\"read\", false)",
    ] {
        let source = edit("@@allow(\"read\", true)", policies);
        assert_eq!(rejected(&source, needle), RESOURCE, "{policies:?}");
    }
    for policies in [
        "@@allow(\"all\", true)",
        "@@allow('read', true)",
        "@@allow(\"list\", true)\n  @@allow(\"detail\", true)",
    ] {
        let source = edit("@@allow(\"read\", true)", policies);
        assert!(
            crate::parse_schema(&source).is_ok(),
            "{policies:?} is a read allow"
        );
    }
}

#[test]
fn rule_tool_needs_an_allow() {
    let needle = "exposes a procedure with no `@allow(...)`";
    let source = edit("  @allow(auth() != null)\n  @mcp(tool)\n", "  @mcp(tool)\n");
    assert_eq!(rejected(&source, needle), "@mcp(tool)");
    let source = edit(
        "  @allow(auth() != null)\n  @mcp(tool)\n",
        "  @deny(false)\n  @mcp(tool)\n",
    );
    assert_eq!(
        rejected(&source, needle),
        "@mcp(tool)",
        "@deny alone never allows"
    );
}

#[test]
fn rule_tool_is_not_a_stream() {
    // `getFeed` returns `Post[]`, so `@stream` alone is valid on it (the
    // `validate_procedure_stream_attribute` arity rule passes); only the
    // combination with `@mcp(tool)` is refused (ADR 0002 Q8).
    let stream = "  @allow(auth() != null)\n  @stream\n  @mcp(tool)\n";
    let source = edit("  @allow(auth() != null)\n  @mcp(tool)\n", stream);
    let span = rejected(&source, "exposes a `@stream` procedure");
    assert_eq!(span, "@mcp(tool)");
    // `publishPost` is still a tool, so `expose = [tools]` stays used.
    let without_tool = source.replacen("  @stream\n  @mcp(tool)\n", "  @stream\n", 1);
    assert!(
        crate::parse_schema(&without_tool).is_ok(),
        "`@stream` on a procedure that is not a tool stays valid"
    );
}
