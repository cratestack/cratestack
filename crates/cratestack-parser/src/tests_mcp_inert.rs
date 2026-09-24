//! An MCP attribute the parser does not read is an error wherever it sits
//! (cratestack#1036, found in review).
//!
//! The parser extracts exactly `@mcp(...)` on its own line under a procedure
//! and `@@mcp(...)` on its own line in a model. Everything else that names
//! MCP — another case (`@MCP`), a space after the `@` (`@ mcp`), a second
//! attribute on the same line in a view or a query, an enum "variant" —
//! used to stay a raw `Attribute` (or variant name) nothing reads: it parsed,
//! and it exposed nothing. A schema whose *only* MCP declaration was
//! `@MCP(tool)` also slipped past the release gate, which reads the typed IR.
//! ADR 0002 § Validation rules out an MCP declaration that is silently inert.

use crate::parse_schema;
use crate::tests_mcp_support::{VALID, edit, rejected};

const NEEDLE: &str = "names MCP but is not an MCP attribute the parser reads";

/// A second, exposable model, so a stray attribute on it leaves `resources`
/// in use and nothing else in the schema is wrong.
fn with_second_model(attribute_line: &str) -> String {
    format!(
        "{VALID}\nmodel Note {{\n  id Int @id\n\n  @@allow(\"read\", true)\n  {attribute_line}\n}}\n"
    )
}

#[test]
fn a_procedure_mcp_in_another_case_or_with_a_space_is_rejected() {
    for spelling in ["@MCP(tool)", "@Mcp(tool)", "@ mcp(tool)", "@\tmcp(tool)"] {
        let source = edit("  @mcp(tool)\n", &format!("  {spelling}\n"));
        assert_eq!(rejected(&source, NEEDLE), spelling, "{spelling}");
    }
}

#[test]
fn a_model_mcp_in_another_case_or_with_a_space_is_rejected() {
    for spelling in ["@@MCP(resource: \"notes\")", "@@ mcp(resource: \"notes\")"] {
        let source = with_second_model(spelling);
        assert_eq!(rejected(&source, NEEDLE), spelling, "{spelling}");
    }
}

#[test]
fn a_misspelled_mcp_cannot_slip_past_the_release_gate_in_a_schema_without_mcp() {
    // No `mcp { }` block and no typed exposure: before this rule the schema
    // parsed cleanly, so `include_server_schema!`'s Q4 gate (which reads the
    // typed IR) saw nothing to reject.
    let source = "procedure ping(n: Int): Int\n  @allow(true)\n  @MCP(tool)\n";
    assert_eq!(rejected(source, NEEDLE), "@MCP(tool)");
}

#[test]
fn a_second_attribute_on_a_procedure_or_model_line_in_another_case_is_rejected() {
    let source = edit(
        "  @allow(auth() != null)\n  @mcp(tool)\n",
        "  @allow(auth() != null) @MCP(tool)\n",
    );
    assert_eq!(
        rejected(&source, NEEDLE),
        "@allow(auth() != null) @MCP(tool)"
    );

    let source = with_second_model("@@deny(\"update\", true) @@Mcp(resource: \"notes\")");
    rejected(&source, NEEDLE);
}

#[test]
fn an_mcp_attribute_sharing_a_view_or_query_line_is_rejected() {
    let view = "view Titles from Post {\n  id Int @id\n\n  \
                @@server_sql(\"SELECT id FROM posts\")\n  \
                @@allow(\"read\", true) @@mcp(resource: \"titles\")\n}\n";
    let source = format!("{VALID}\n{view}");
    assert_eq!(
        rejected(&source, NEEDLE),
        "@@allow(\"read\", true) @@mcp(resource: \"titles\")"
    );

    let query = "type Row {\n  n Int\n}\n\nquery rows(userId: String): Row\n  \
                 @@sql(\"SELECT 1 AS n WHERE a = $1\")\n  @allow(true) @mcp(tool)\n";
    let source = format!("{VALID}\n{query}");
    assert_eq!(rejected(&source, NEEDLE), "@allow(true) @mcp(tool)");
}

#[test]
fn an_mcp_attribute_glued_to_or_misspelled_on_a_field_is_rejected() {
    for attributes in ["@unique@mcp(tool)", "@Mcp(tool)"] {
        let source = edit(
            "  title String\n",
            &format!("  title String {attributes}\n"),
        );
        rejected(&source, NEEDLE);
    }
}

#[test]
fn an_mcp_line_inside_an_enum_is_rejected_not_read_as_a_variant() {
    for line in ["@@mcp", "@mcp"] {
        let source = format!("{VALID}\nenum Role {{\n  admin\n  {line}\n}}\n");
        assert_eq!(rejected(&source, NEEDLE), line, "{line}");
    }
}

#[test]
fn mcp_inside_a_string_or_as_a_longer_name_is_not_a_false_positive() {
    let source = edit(
        "description: \"Publish a draft, then notify: now.\"",
        "description: \"Reach it as @MCP(tool) or @@mcp.\"",
    );
    parse_schema(&source).expect("text inside a string literal is not an attribute");

    let source = edit(
        "  title String\n",
        "  title String @default(\"@mcp(tool)\")\n",
    );
    parse_schema(&source).expect("a string default is not an attribute");
}
