//! The `mcp { }` block (cratestack#1036): `key = value` like every other
//! config block, with `expose = [tools, resources]` its only key (maintainer,
//! 2026-09-24). Before this, the body was raw text that nothing read, so
//! `mcp { anything }` parsed.

use crate::parse_schema;
use crate::tests_mcp_support::{VALID, edit, rejected, syntax_error};

const EXPOSE: &str = "  expose = [tools, resources]\n";

#[test]
fn either_kind_alone_is_a_complete_block() {
    let schema = parse_schema(
        &edit(EXPOSE, "  expose = [tools]\n")
            .replace("  @@mcp(resource: \"posts\", max_page_size: 20)\n", ""),
    )
    .expect("tools only");
    let mcp = schema.mcp.expect("typed block");
    assert!(mcp.exposes_tools() && !mcp.exposes_resources());
}

#[test]
fn spacing_inside_the_list_is_free_and_element_spans_follow_it() {
    let source = edit(EXPOSE, "  expose=[ resources ,tools ]\n");
    let mcp = parse_schema(&source).expect("parses").mcp.expect("block");
    let tools = mcp.expose_tools.expect("tools");
    let resources = mcp.expose_resources.expect("resources");
    assert_eq!(&source[tools.start..tools.end], "tools");
    assert_eq!(&source[resources.start..resources.end], "resources");
}

#[test]
fn malformed_expose_elements_are_errors_at_the_element() {
    let cases = [
        ("[procedures]", "renamed to `tools`", "procedures"),
        (
            "[tools, prompts]",
            "unknown `expose` element `prompts`",
            "prompts",
        ),
        ("[tools, tools]", "listed more than once", "tools"),
        ("[tools,]", "empty element", ""),
    ];
    for (list, needle, span) in cases {
        let source = edit(EXPOSE, &format!("  expose = {list}\n"));
        assert_eq!(rejected(&source, needle), span, "{list}");
    }
    // The duplicate points at the second `tools`, not the first.
    let source = edit(EXPOSE, "  expose = [tools, tools]\n");
    let (_, errors) = crate::parse_schema_diagnostics("t", &source);
    assert_eq!(errors[0].span().start, source.find("tools]").unwrap());
}

#[test]
fn malformed_expose_values_and_keys_are_errors() {
    let cases = [
        ("  expose = []\n", "`expose = []` is empty"),
        ("  expose = tools\n", "must be a one-line list"),
        ("  expose = [tools\n", "must be a one-line list"),
        (
            "  expose = [tools, resources]\n  expose = [tools]\n",
            "`expose` is set more than once",
        ),
        (
            "  expose = [tools, resources]\n  prompts = [a]\n",
            "unknown `mcp` setting `prompts`",
        ),
        ("  tools = true\n", "unknown `mcp` setting `tools`"),
        ("  exposing everything\n", "unsupported `mcp` block entry"),
    ];
    for (to, needle) in cases {
        let message = syntax_error(&edit(EXPOSE, to));
        assert!(message.contains(needle), "{to:?}: {message}");
    }
}

/// The migration hint: the line form ADR 0002's first revision used is
/// rejected with the new spelling in the message.
#[test]
fn the_old_line_form_is_rejected_with_the_new_spelling() {
    let message = syntax_error(&edit(EXPOSE, "  expose tools\n  expose resources\n"));
    assert!(
        message.contains("`expose tools` is the old line form"),
        "{message}"
    );
    assert!(message.contains("write `expose = [tools]`"), "{message}");
    assert!(
        message.contains("`expose = [tools, resources]`"),
        "{message}"
    );
    let message = syntax_error(&edit(EXPOSE, "  expose procedures\n"));
    assert!(message.contains("write `expose = [tools]`"), "{message}");
}

#[test]
fn a_block_without_expose_and_block_structure_errors() {
    // Alone, with no attribute to trip any other rule: a block with no
    // `expose` would otherwise be a valid, inert declaration.
    let empty = syntax_error("mcp {\n  // nothing yet\n}\n");
    assert!(empty.contains("has no `expose` key"), "{empty}");
    let twice = format!("{VALID}\nmcp {{\n  expose = [tools]\n}}\n");
    assert!(syntax_error(&twice).contains("duplicate `mcp { }` block"));
    let unterminated = "mcp {\n  expose = [tools]\n";
    assert!(syntax_error(unterminated).contains("unterminated `mcp` block"));
}
