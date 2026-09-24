//! Structure of what the generator *does* map: which return types get an
//! output schema, what is required, how declarations are shared. Whether
//! the mapped schemas agree with serde is the round-trip suites' job
//! (`cratestack-api`/`cratestack-pg` `tests/json_schema_*.rs`); these only
//! pin the IR-side decisions that those suites can't observe directly.

use serde_json::{Value, json};

use super::tests::parse;
use super::{DIALECT, procedure_input_schema, procedure_output_schema};
use crate::shared::decimal_backend::DecimalBackend;

const DECLS: &str = "enum Status {\n  Active\n  Archived\n}\n\
    type Item {\n  /// Shown to agents.\n  label String\n  note String?\n  tags String[]\n}\n\
    type Tree {\n  label String\n  children Tree[]\n}\n";

fn input(source: &str) -> Value {
    let schema = parse(source);
    procedure_input_schema(
        &schema,
        &schema.procedures[0],
        Some(DecimalBackend::RustDecimal),
    )
    .expect("input schema")
}

fn output(source: &str) -> Option<Value> {
    let schema = parse(source);
    procedure_output_schema(&schema, &schema.procedures[0], None).expect("output schema")
}

#[test]
fn only_object_return_types_get_an_output_schema() {
    for (returns, expected) in [
        ("Item", true),
        ("Page<Item>", true),
        ("Item?", false),
        ("Item[]", false),
        ("Status", false),
        ("Int", false),
        ("String[]", false),
    ] {
        let schema = output(&format!("{DECLS}procedure p(): {returns}"));
        assert_eq!(schema.is_some(), expected, "return type {returns}");
        if let Some(schema) = schema {
            assert_eq!(schema["type"], "object", "return type {returns}");
            assert_eq!(schema["$schema"], DIALECT);
        }
    }
}

#[test]
fn arguments_are_required_unless_optional() {
    let schema = input(&format!(
        "{DECLS}procedure p(item: Item, maybe: Item?, many: Status[], page: PageInput?): Int"
    ));
    assert_eq!(schema["$schema"], DIALECT);
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["required"], json!(["item", "many"]));
    assert_eq!(
        schema["properties"]["maybe"],
        json!({ "anyOf": [{ "$ref": "#/$defs/Item" }, { "type": "null" }] })
    );
    assert_eq!(
        schema["$defs"]["Item"]["required"],
        json!(["label", "tags"])
    );
    assert_eq!(
        schema["$defs"]["Item"]["properties"]["label"]["description"],
        "Shown to agents."
    );
    assert_eq!(
        schema["$defs"]["Status"],
        json!({ "type": "string", "enum": ["Active", "Archived"] })
    );
}

#[test]
fn a_procedure_without_arguments_takes_an_empty_object() {
    let schema = input(&format!("{DECLS}procedure p(): Int"));
    assert_eq!(schema["properties"], json!({}));
    assert!(schema.get("required").is_none());
    assert!(schema.get("$defs").is_none());
}

#[test]
fn a_self_referencing_type_terminates_through_defs() {
    let schema = input(&format!("{DECLS}procedure p(tree: Tree): Int"));
    assert_eq!(
        schema["$defs"]["Tree"]["properties"]["children"],
        json!({ "type": "array", "items": { "$ref": "#/$defs/Tree" } })
    );
}

#[test]
fn model_relations_and_server_only_fields_are_not_advertised() {
    let schema = parse(
        "model Author {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  title String\n  secret String @server_only\n\
         authorId Int\n  author Author @relation(fields: [authorId], references: [id])\n}\n\
         procedure p(): Post",
    );
    let schema = procedure_output_schema(&schema, &schema.procedures[0], None)
        .expect("output schema")
        .expect("model output is an object");
    let post = &schema["$defs"]["Post"];
    let mut names: Vec<&str> = post["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["authorId", "id", "title"]);
    assert!(schema["$defs"].get("Author").is_none());
}

#[test]
fn decimal_schema_follows_the_selected_backend() {
    let schema = parse("procedure p(amount: Decimal): Int");
    let pattern = |backend| {
        procedure_input_schema(&schema, &schema.procedures[0], Some(backend)).unwrap()["properties"]
            ["amount"]["pattern"]
            .clone()
    };
    let rust = pattern(DecimalBackend::RustDecimal);
    let big = pattern(DecimalBackend::BigDecimal);
    assert_ne!(rust, big, "the backends serialize differently");
    assert!(!rust.as_str().unwrap().contains("eE"));
    assert!(big.as_str().unwrap().contains("[eE]"));
}
