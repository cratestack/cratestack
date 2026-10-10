//! `BigInt` (ADR 0019) in generated stubs.
//!
//! A `BigInt` is a canonical decimal string on the wire, never a JSON
//! number: the generated TypeScript client throws on a number at a
//! `BigInt` key and the Dart client calls `BigInt.parse` on a `String`.
//! A stub that renders `0` instead of `"0"` is therefore a mock that no
//! generated client can read. Like `tests/models.rs`, these assert on the
//! *shape* of the generated stubs; the live round trip against a real
//! `wiremock-state-extension` container is not exercised by this crate's
//! test suite.

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

fn body(package: &GeneratedWireMockPackage, file_name: &str) -> String {
    mapping(package, file_name)["response"]["body"]
        .as_str()
        .unwrap_or_else(|| panic!("{file_name}: expected a templated `response.body` string"))
        .to_owned()
}

const NO_DATASOURCE: &str = "datasource db {
  provider = \"none\"
}
";

const PG_DATASOURCE: &str = "datasource db {
  provider = \"postgresql\"
  url = env(\"DATABASE_URL\")
}
";

#[test]
fn procedure_stub_renders_every_bigint_shape_as_a_decimal_string() {
    let package = package(&format!(
        "{NO_DATASOURCE}
type Balance {{
  amountE8 BigInt
  feeE8 BigInt?
  history BigInt[]
  hits Int
}}

procedure balance(): Balance
"
    ));
    let mapping = mapping(&package, "mappings/balance.json");
    let body = &mapping["response"]["jsonBody"];

    assert_eq!(body["amountE8"], serde_json::json!("0"));
    assert_eq!(body["feeE8"], serde_json::json!("0"));
    assert_eq!(body["history"], serde_json::json!(["0"]));
    // `Int` is untouched: still a bare JSON number.
    assert_eq!(body["hits"], serde_json::json!(0));
}

#[test]
fn bare_bigint_procedure_return_is_a_decimal_string() {
    let package = package(&format!("{NO_DATASOURCE}\nprocedure total(): BigInt\n"));
    let mapping = mapping(&package, "mappings/total.json");
    assert_eq!(mapping["response"]["jsonBody"], serde_json::json!("0"));
}

#[test]
fn stateful_rest_stub_quotes_a_bigint_field_and_leaves_int_bare() {
    let package = package(&format!(
        "{PG_DATASOURCE}
model Ledger {{
  id Int @id
  amountE8 BigInt
  hits Int
}}
"
    ));

    for verb in ["create", "get", "update", "delete"] {
        let body = body(&package, &format!("mappings/model.Ledger.{verb}.json"));
        assert!(
            body.contains("\"amountE8\": \"{{"),
            "{verb}: a BigInt field must be rendered inside quotes: {body}"
        );
        assert!(
            body.contains("\"hits\": {{"),
            "{verb}: an Int field stays a bare number: {body}"
        );
    }
    let create = body(&package, "mappings/model.Ledger.create.json");
    assert!(
        create.contains("}}0{{else}}"),
        "the create-time fallback for a BigInt is the canonical `0`: {create}"
    );
}

#[test]
fn bigint_primary_key_is_generated_as_a_canonical_decimal_string() {
    let package = package(&format!(
        "{PG_DATASOURCE}
model Ledger {{
  id BigInt @id
  note String
}}
"
    ));
    let create = body(&package, "mappings/model.Ledger.create.json");
    assert!(
        create.starts_with("{ \"id\": \"1{{randomValue length=5 type='NUMERIC'}}\""),
        "a BigInt id must be a quoted, non-zero-leading digit string: {create}"
    );
    assert!(
        !create.contains("ALPHANUMERIC"),
        "a BigInt id must not be an alphanumeric string: {create}"
    );
    let get = body(&package, "mappings/model.Ledger.get.json");
    assert!(
        get.starts_with("{ \"id\": \"{{state context=request.path property='id'}}\""),
        "{get}"
    );
}

#[test]
fn bigint_version_is_a_quoted_string_in_every_body_and_int_version_stays_bare() {
    let package = package(&format!(
        "{PG_DATASOURCE}
model Ledger {{
  id Int @id
  version BigInt @version
}}

model Plain {{
  id Int @id
  version Int @version
}}
"
    ));

    let create = body(&package, "mappings/model.Ledger.create.json");
    assert!(create.contains("\"version\": \"0\""), "{create}");
    let get = body(&package, "mappings/model.Ledger.get.json");
    assert!(
        get.contains("\"version\": \"{{state context=request.path property='version'}}\""),
        "{get}"
    );
    let update = body(&package, "mappings/model.Ledger.update.json");
    assert!(
        update.contains(
            "\"version\": \"{{math (state context=request.path property='version') '+' 1}}\""
        ),
        "{update}"
    );

    // Contrast: an `Int` version is unchanged.
    let plain_create = body(&package, "mappings/model.Plain.create.json");
    assert!(plain_create.contains("\"version\": 0"), "{plain_create}");
    let plain_update = body(&package, "mappings/model.Plain.update.json");
    assert!(
        plain_update.contains(
            "\"version\": {{math (state context=request.path property='version') '+' 1}}"
        ),
        "{plain_update}"
    );
}

#[test]
fn rpc_transport_model_stub_renders_bigint_as_a_decimal_string() {
    let package = package(&format!(
        "transport rpc

{PG_DATASOURCE}
model Ledger {{
  id BigInt @id
  amountE8 BigInt
  hits Int
}}
"
    ));
    let mapping = mapping(&package, "mappings/model.Ledger.create.json");
    let body = &mapping["response"]["jsonBody"];
    assert_eq!(body["id"], serde_json::json!("0"));
    assert_eq!(body["amountE8"], serde_json::json!("0"));
    assert_eq!(body["hits"], serde_json::json!(0));
}
