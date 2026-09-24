//! `name = "..."` in the `mcp { }` block (maintainer decision on
//! cratestack#1040): the `<name>` of every resource URI. One test per rule,
//! each derived from [`VALID`] by one edit, so deleting that rule's check
//! makes exactly that test fail.

use crate::parse_schema;
use crate::tests_mcp_support::{VALID, edit, rejected, syntax_error};

const NAME: &str = "  name = \"blog\"\n";

#[test]
fn the_name_is_carried_into_the_ir_with_its_entry_span() {
    let mcp = parse_schema(VALID).expect("valid").mcp.expect("block");
    let name = mcp.name.expect("a name");
    assert_eq!(name.value, "blog");
    assert_eq!(&VALID[name.span.start..name.span.end], "name = \"blog\"");

    // Any `[a-z0-9-]+`, and anywhere in the block.
    let source = edit(NAME, "").replace(
        "  expose = [tools, resources]\n",
        "  expose = [tools, resources]\n  name = \"my-app-2\"\n",
    );
    let name = parse_schema(&source).expect("valid").mcp.unwrap().name;
    assert_eq!(name.unwrap().value, "my-app-2");
}

#[test]
fn rule_resources_need_a_name() {
    let source = edit(NAME, "");
    let span = rejected(
        &source,
        "`expose` lists `resources`, but the block has no `name`",
    );
    assert_eq!(span, "resources", "points at what asks for a name");
}

#[test]
fn rule_a_name_needs_resources() {
    let source = edit("  expose = [tools, resources]\n", "  expose = [tools]\n")
        .replace("  @@mcp(resource: \"posts\", max_page_size: 20)\n", "");
    let span = rejected(&source, "so it would name nothing");
    assert_eq!(span, "name = \"blog\"");
}

#[test]
fn rule_the_name_is_a_quoted_string() {
    for value in ["blog", "'blog'", "\"blog", "env(\"NAME\")"] {
        let message = syntax_error(&edit(NAME, &format!("  name = {value}\n")));
        assert!(
            message.contains("must be a quoted string"),
            "{value}: {message}"
        );
    }
}

#[test]
fn rule_the_name_is_lowercase_letters_digits_and_hyphens() {
    for value in [
        "Blog", "BLOG", "my_app", "my.app", "my app", "", "café", "a/b", "a%20",
    ] {
        let message = syntax_error(&edit(NAME, &format!("  name = \"{value}\"\n")));
        assert!(
            message.contains("must be lowercase ASCII letters, digits and `-` only"),
            "{value:?}: {message}"
        );
    }
}

#[test]
fn rule_the_name_is_set_once() {
    let message = syntax_error(&edit(NAME, "  name = \"blog\"\n  name = \"blog\"\n"));
    assert!(
        message.contains("`name` is set more than once"),
        "{message}"
    );
    let message = syntax_error(&edit(NAME, "  name = \"blog\"\n  name = \"other\"\n"));
    assert!(
        message.contains("`name` is set more than once"),
        "{message}"
    );
}
