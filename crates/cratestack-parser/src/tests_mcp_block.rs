//! The `mcp { }` block (cratestack#1036): every line is one of the two
//! `expose` settings or an error. Before this, the body was raw text that
//! nothing read, so `mcp { anything }` parsed.

use crate::parse_schema;
use crate::tests_mcp_support::{VALID, edit, syntax_error};

#[test]
fn either_expose_line_alone_is_a_complete_block() {
    let schema = parse_schema(
        &edit("  expose resources\n", "")
            .replace("  @@mcp(resource: \"posts\", max_page_size: 20)\n", ""),
    )
    .expect("tools only");
    let mcp = schema.mcp.expect("typed block");
    assert!(mcp.exposes_tools() && !mcp.exposes_resources());
}

#[test]
fn malformed_blocks_are_parse_errors() {
    let cases = [
        (
            "  expose tools\n",
            "  expose procedures\n",
            "renamed to `expose tools`",
        ),
        (
            "  expose tools\n",
            "  expose prompts\n",
            "unknown `expose` target",
        ),
        (
            "  expose tools\n",
            "  expose tools\n  expose tools\n",
            "more than once",
        ),
        (
            "  expose tools\n",
            "  tools = true\n",
            "unsupported `mcp` block entry",
        ),
    ];
    for (from, to, needle) in cases {
        let message = syntax_error(&edit(from, to));
        assert!(message.contains(needle), "{to:?}: {message}");
    }
    // Alone, with no attribute to trip any other rule: an empty block would
    // otherwise be a valid, inert declaration.
    let empty = syntax_error("mcp {\n  // nothing yet\n}\n");
    assert!(empty.contains("empty `mcp { }` block"), "{empty}");
    let twice = format!("{VALID}\nmcp {{\n  expose tools\n}}\n");
    assert!(syntax_error(&twice).contains("duplicate `mcp { }` block"));
    let unterminated = "mcp {\n  expose tools\n";
    assert!(syntax_error(unterminated).contains("unterminated `mcp` block"));
}
