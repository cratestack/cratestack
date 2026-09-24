//! The resource refusals of `resources.rs` (cratestack#1040), in both
//! feature states: each is decided before the feature check, so an author
//! hears about a resource that can never be served before being told to
//! turn a feature on for it.

use super::plan::server_plan;

fn schema(model_body: &str) -> cratestack_core::Schema {
    let source = format!(
        r#"
datasource db {{
  provider = "postgresql"
}}

mcp {{
  name = "blog"
  expose = [resources]
}}

model Post {{
{model_body}

  @@allow("read", true)
}}
"#
    );
    cratestack_parser::parse_schema(&source).expect("valid schema")
}

fn refused(schema: &cratestack_core::Schema) -> String {
    let mut messages = [false, true].map(|feature| {
        server_plan(schema, None, feature)
            .err()
            .unwrap_or_else(|| panic!("refused with the feature {feature}"))
    });
    assert_eq!(messages[0], messages[1], "the same refusal in both states");
    std::mem::take(&mut messages[0])
}

#[test]
fn max_page_size_is_carried_into_the_plan() {
    let schema = schema("  id Int @id\n  @@mcp(resource: \"posts\", max_page_size: 20)");
    let plan = server_plan(&schema, None, true).expect("served");
    assert_eq!(plan.resources[0].max_page_size, 20);
}

/// `@@internal` speaks `@@allow`'s vocabulary; `model_internal_actions`
/// expands it to the wire verbs, so `detail`, `read` and `all` all hide
/// `get`, and `list`, `read` and `all` hide `list`.
#[test]
fn an_internal_read_verb_contradicts_the_resource() {
    for (action, verb) in [
        ("detail", "get"),
        ("list", "list"),
        ("read", "get"),
        ("all", "get"),
    ] {
        let schema = schema(&format!(
            "  id Int @id\n  @@internal(\"{action}\")\n  @@mcp(resource: \"posts\")"
        ));
        let message = refused(&schema);
        assert!(
            message.contains(&format!("keeps the model's `{verb}` off the wire")),
            "{action}: {message}"
        );
    }
}

#[test]
fn a_write_only_internal_verb_is_no_contradiction() {
    let schema = schema("  id Int @id\n  @@internal(\"create\")\n  @@mcp(resource: \"posts\")");
    assert!(server_plan(&schema, None, true).is_ok());
}

#[test]
fn a_key_a_uri_cannot_carry_is_refused() {
    let schema = schema("  id DateTime @id\n  @@mcp(resource: \"posts\")");
    let message = refused(&schema);
    assert!(
        message.contains("cannot address a record by URI"),
        "{message}"
    );
    assert!(message.contains("`DateTime`"), "{message}");
}

#[test]
fn every_addressable_key_type_is_planned() {
    for ty in ["String", "Cuid", "Int", "Uuid"] {
        let schema = schema(&format!("  id {ty} @id\n  @@mcp(resource: \"posts\")"));
        assert!(server_plan(&schema, None, true).is_ok(), "{ty}");
    }
}

/// The authority is the block's `name` (maintainer decision on
/// cratestack#1040), which replaced the `.cstack` file stem: the plan no
/// longer sees the file at all, so no rename can move a URI.
#[test]
fn the_authority_is_the_blocks_name() {
    let mut schema = schema("  id Int @id\n  @@mcp(resource: \"posts\")");
    let plan = server_plan(&schema, None, true).unwrap();
    assert_eq!(plan.resources[0].authority, "blog");

    // The parser never lets this through; the plan refuses it anyway
    // rather than inventing an authority.
    schema.mcp.as_mut().unwrap().name = None;
    let message = refused(&schema);
    assert!(message.contains("has no `name = \"...\"`"), "{message}");
}
