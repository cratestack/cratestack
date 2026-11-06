//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) in
//! generated stubs. An ordinal enum is a JSON number on the wire, so an
//! example is its first variant's ordinal, never the name. A model field
//! of one is frozen to that example rather than echoed from the request:
//! a TypeScript client sends the name and a Rust or Dart one the number,
//! and no single quoting renders both as valid JSON.

use cratestack_mock_wiremock::{
    GeneratedWireMockPackage, WireMockGeneratorConfig, generate_package,
};

fn package(source: &str) -> GeneratedWireMockPackage {
    let schema = cratestack_parser::parse_schema(source).expect("schema should parse");
    generate_package(&schema, &WireMockGeneratorConfig::default()).expect("generation succeeds")
}

fn mapping(package: &GeneratedWireMockPackage, file_name: &str) -> serde_json::Value {
    let file = package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("no generated file named {file_name}"));
    serde_json::from_str(&file.contents).expect("generated file should be valid JSON")
}

// `Approved` is first and numbered 2, so an index would not pass for it.
const ENUMS: &str = "enum Status {
  Approved = 2
  Pending = 1
}

enum Channel {
  card
  transfer
}
";

#[test]
fn a_procedure_example_is_the_first_variants_ordinal() {
    let package = package(&format!(
        "datasource db {{\n  provider = \"none\"\n}}\n{ENUMS}
type Summary {{
  current Status
  history Status[]
  channel Channel
}}

procedure summary(): Summary
procedure statusOf(): Status
"
    ));
    let body = &mapping(&package, "mappings/summary.json")["response"]["jsonBody"];
    assert_eq!(body["current"], serde_json::json!(2));
    assert_eq!(body["history"], serde_json::json!([2]));
    assert_eq!(body["channel"], serde_json::json!("card"));
    let bare = &mapping(&package, "mappings/statusOf.json")["response"]["jsonBody"];
    assert_eq!(bare, &serde_json::json!(2));
}

#[test]
fn a_model_field_is_frozen_to_its_ordinal_and_a_string_enum_is_still_echoed() {
    let package = package(&format!(
        "datasource db {{\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}}\n{ENUMS}
model Payment {{
  id Int @id
  status Status
  channel Channel
}}
"
    ));
    for verb in ["create", "get", "update"] {
        let mapping = mapping(&package, &format!("mappings/model.Payment.{verb}.json"));
        let body = mapping["response"]["body"]
            .as_str()
            .unwrap_or_else(|| panic!("{verb}: expected a templated body"));
        assert!(body.contains("\"status\": 2"), "{verb}: {body}");
        assert!(body.contains("\"channel\": \"{{"), "{verb}: {body}");
    }
}
