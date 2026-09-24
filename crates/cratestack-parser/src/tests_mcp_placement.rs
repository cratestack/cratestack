//! `@mcp`/`@@mcp` in a position the parser does not extract it from
//! (cratestack#1036). Without these rejections the attribute would stay a raw
//! `Attribute` nothing reads — silently inert, the outcome ADR 0002 §
//! Validation rules out.

use crate::tests_mcp_support::{edit, rejected, syntax_error};

#[test]
fn mcp_on_a_model_field_is_rejected() {
    for attribute in ["@mcp(tool)", "@@mcp(resource: \"titles\")"] {
        let source = edit("  title String\n", &format!("  title String {attribute}\n"));
        let span = rejected(&source, "never on a field");
        assert_eq!(span, attribute);
    }
}

#[test]
fn mcp_on_every_other_field_bearing_declaration_is_rejected() {
    let declarations = [
        ("type FeedArgs {\n  limit Int", "type"),
        (
            "type FeedArgs {\n  limit Int\n}\n\nmixin Stamped {\n  at String",
            "mixin",
        ),
        (
            "type FeedArgs {\n  limit Int\n}\n\nauth Session {\n  id String",
            "auth block",
        ),
    ];
    for (header, kind) in declarations {
        let source = edit(
            "type FeedArgs {\n  limit Int",
            &format!("{header} @mcp(tool)"),
        );
        let span = rejected(&source, &format!("on {kind} `"));
        assert_eq!(span, "@mcp(tool)", "{kind}");
    }
}

#[test]
fn mcp_on_a_view_or_a_view_field_is_rejected() {
    let view = "view PostTitles from Post {\n  id Int @id\n  title String\n\n  \
                @@allow(\"read\", true)\n  @@server_sql(\"SELECT id, title FROM posts\")\n";
    let source = edit(
        "procedure getFeed",
        &format!("{view}  @@mcp(resource: \"titles\")\n}}\n\nprocedure getFeed"),
    );
    let span = rejected(&source, "only a model can be an MCP resource");
    assert_eq!(span, "@@mcp(resource: \"titles\")");

    let with_field = view.replace("  title String\n", "  title String @mcp(tool)\n");
    let source = edit(
        "procedure getFeed",
        &format!("{with_field}}}\n\nprocedure getFeed"),
    );
    assert_eq!(rejected(&source, "on view `PostTitles`"), "@mcp(tool)");
}

#[test]
fn model_mcp_on_a_procedure_is_rejected_with_a_pointer_to_mcp_tool() {
    let message = syntax_error(&edit("  @mcp(tool)\n", "  @@mcp(resource: \"feed\")\n"));
    assert!(
        message.contains("a procedure is exposed as an MCP tool with `@mcp(tool)`"),
        "{message}"
    );
}

#[test]
fn single_at_mcp_inside_a_model_body_is_rejected() {
    let message = syntax_error(&edit(
        "  @@mcp(resource: \"posts\", max_page_size: 20)",
        "  @mcp(tool)",
    ));
    assert!(
        message.contains("unsupported model directive `@mcp(tool)`"),
        "{message}"
    );
}

#[test]
fn an_mcp_attribute_on_a_query_is_rejected() {
    let source = edit(
        "procedure getFeed",
        "type Row {\n  n Int\n}\n\nquery rows(userId: String): Row\n  \
         @@sql(\"SELECT 1 AS n WHERE a = $1\")\n  @allow(true)\n  @mcp(tool)\n\n\
         procedure getFeed",
    );
    assert!(rejected(&source, "unsupported attribute `@mcp").starts_with("@mcp"));
}
