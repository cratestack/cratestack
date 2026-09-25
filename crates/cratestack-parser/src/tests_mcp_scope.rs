//! ADR 0002 § Validation, the rules about scope (cratestack#1036): does the
//! `mcp { expose = [...] }` block agree with the attributes, and can the
//! exposed kinds exist at all. Same conventions as `tests_mcp_rules`.

use crate::tests_mcp_support::{edit, rejected};

const TOOL: &str =
    r#"@mcp(tool: "publish_post", description: "Publish a draft, then notify: now.")"#;
const RESOURCE: &str = r#"@@mcp(resource: "posts", max_page_size: 20)"#;
const BLOCK: &str = "mcp {\n  name = \"blog\"\n  expose = [tools, resources]\n}\n";
const EXPOSE: &str = "expose = [tools, resources]";

fn without_block() -> String {
    edit(BLOCK, "")
}

#[test]
fn rule_attribute_without_a_block() {
    let source = without_block();
    let message = "an MCP attribute in a schema with no `mcp { }` block is an error";
    // Both attribute kinds report, each at its own attribute.
    assert_eq!(
        rejected(&source, "procedure `getFeed` needs a top-level"),
        "@mcp(tool)"
    );
    assert_eq!(
        rejected(&source, "model `Post` needs a top-level"),
        RESOURCE
    );
    rejected(&source, message);
}

#[test]
fn rule_attribute_without_its_exposed_kind() {
    let source = edit(EXPOSE, "expose = [resources]");
    let span = rejected(&source, "`expose` list has no `tools`");
    assert_eq!(span, "@mcp(tool)");
    let source = edit(EXPOSE, "expose = [tools]");
    assert_eq!(
        rejected(&source, "`expose` list has no `resources`"),
        RESOURCE
    );
}

#[test]
fn rule_unused_exposed_tools() {
    let source = edit("  @mcp(tool)\n", "").replace(&format!("  {TOOL}\n"), "");
    assert_eq!(
        rejected(&source, "`tools` in `expose` exposes nothing"),
        "tools"
    );
}

#[test]
fn rule_unused_exposed_resources() {
    let source = edit(&format!("  {RESOURCE}\n"), "");
    assert_eq!(
        rejected(&source, "`resources` in `expose` exposes nothing"),
        "resources"
    );
}

#[test]
fn rule_no_resources_without_a_database() {
    // `provider = "none"` is `db = None`. The pre-existing "no `model` under
    // `provider = \"none\"`" error also fires for `Post`; these two are the
    // MCP-specific ones, reported alongside it.
    let source = edit("provider = \"postgresql\"", "provider = \"none\"");
    assert_eq!(
        rejected(
            &source,
            "`resources` in `expose` is not allowed: a `datasource"
        ),
        "resources"
    );
    assert_eq!(
        rejected(
            &source,
            "`@@mcp` on model `Post` is not allowed: a `datasource"
        ),
        RESOURCE
    );
    rejected(&source, "model `Post` is not allowed: schema declares");
}
