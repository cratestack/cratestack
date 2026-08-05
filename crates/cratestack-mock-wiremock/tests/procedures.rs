use cratestack_mock_wiremock::{WireMockGeneratorConfig, WireMockGeneratorError, generate_package};

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

const NO_DATASOURCE: &str = "datasource db {
  provider = \"none\"
}
";

#[test]
fn generates_one_mapping_file_per_procedure_under_mappings() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Greeting {{
  message String
}}

type FarewellArgs {{
  name String
}}

procedure hello(): Greeting
mutation procedure goodbye(args: FarewellArgs): Greeting
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generation should succeed");

    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["mappings/goodbye.json", "mappings/hello.json"]);
}

#[test]
fn rest_procedure_stub_matches_the_dollar_procs_route_and_default_base_path() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Greeting {{
  message String
}}

procedure hello(): Greeting
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generation should succeed");
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();

    assert_eq!(mapping["request"]["method"], "POST");
    assert_eq!(mapping["request"]["urlPath"], "/api/$procs/hello");
    assert_eq!(mapping["response"]["status"], 200);
    assert_eq!(
        mapping["response"]["headers"]["Content-Type"],
        "application/json"
    );
    assert_eq!(mapping["response"]["jsonBody"]["message"], "string");
    assert_eq!(mapping["metadata"]["cratestack"]["procedure"], "hello");
    assert_eq!(mapping["metadata"]["cratestack"]["kind"], "query");
}

#[test]
fn mutation_procedure_is_labeled_as_such_in_metadata() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type FarewellArgs {{
  name String
}}

type Ack {{
  ok Boolean
}}

mutation procedure goodbye(args: FarewellArgs): Ack
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();

    assert_eq!(mapping["metadata"]["cratestack"]["kind"], "mutation");
    assert_eq!(mapping["response"]["jsonBody"]["ok"], true);
}

#[test]
fn rpc_transport_uses_the_rpc_route_instead_of_dollar_procs() {
    let schema = schema(&format!(
        "transport rpc

{NO_DATASOURCE}
type Greeting {{
  message String
}}

procedure hello(): Greeting
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();

    assert_eq!(mapping["request"]["urlPath"], "/api/rpc/hello");
}

#[test]
fn custom_base_path_is_honored_and_trailing_slash_is_trimmed() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Greeting {{
  message String
}}

procedure hello(): Greeting
"
    ));

    let config = WireMockGeneratorConfig {
        base_path: "/rust-bff/".to_owned(),
    };
    let package = generate_package(&schema, &config).unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();

    assert_eq!(mapping["request"]["urlPath"], "/rust-bff/$procs/hello");
}

#[test]
fn synthesizes_nested_types_lists_optionals_and_enums() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
enum Status {{
  Active
  Inactive
}}

type Item {{
  id Uuid
  label String?
  status Status
}}

type Reply {{
  items Item[]
  total Int
}}

procedure listItems(): Reply
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();
    let body = &mapping["response"]["jsonBody"];

    assert_eq!(body["total"], 0);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "00000000-0000-0000-0000-000000000000");
    assert_eq!(items[0]["label"], "string");
    assert_eq!(items[0]["status"], "Active");
}

#[test]
fn synthesizes_page_envelope_for_page_return_types() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Item {{
  id Int
}}

procedure listItems(): Page<Item>
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();
    let body = &mapping["response"]["jsonBody"];

    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["id"], 0);
    assert_eq!(body["totalCount"], 1);
    assert_eq!(body["pageInfo"]["hasNextPage"], false);
    assert_eq!(body["pageInfo"]["hasPreviousPage"], false);
}

#[test]
fn optional_self_reference_terminates_with_null_instead_of_looping_forever() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Node {{
  value Int
  next Node?
}}

procedure getNode(): Node
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("optional cycle should terminate instead of erroring");
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();
    let body = &mapping["response"]["jsonBody"];

    assert_eq!(body["value"], 0);
    assert!(body["next"].is_null());
}

#[test]
fn list_self_reference_terminates_with_an_empty_array() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type Category {{
  name String
  children Category[]
}}

procedure getCategory(): Category
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("list cycle should terminate instead of erroring");
    let mapping: serde_json::Value = serde_json::from_str(&package.files[0].contents).unwrap();
    let body = &mapping["response"]["jsonBody"];

    assert_eq!(body["name"], "string");
    // `Category` is already "in progress" (its own fields are being
    // synthesized) by the time `children: Category[]` is reached, so the
    // cycle guard breaks it immediately — no nesting is synthesized at
    // all, not even one level. Still a real, useful `Category` instance:
    // just one with no children in the example.
    assert_eq!(body["children"], serde_json::json!([]));
}

#[test]
fn required_self_reference_is_a_hard_error_not_infinite_recursion() {
    let schema = schema(&format!(
        "{NO_DATASOURCE}
type A {{
  b B
}}

type B {{
  a A
}}

procedure getA(): A
"
    ));

    let error = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect_err("an unbreakable required cycle must error, not hang or overflow the stack");
    assert!(matches!(
        error,
        WireMockGeneratorError::UnbreakableCycle { .. }
    ));
}

#[test]
fn grpc_transport_is_rejected_up_front() {
    let schema = schema(&format!(
        "transport grpc

{NO_DATASOURCE}
type Greeting {{
  message String
}}

procedure hello(): Greeting
"
    ));

    let error = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect_err("grpc transport is out of scope for v1");
    assert!(matches!(
        error,
        WireMockGeneratorError::UnsupportedTransport
    ));
}

#[test]
fn schema_with_no_procedures_generates_no_files() {
    let schema = schema(
        "datasource db {
  provider = \"sqlite\"
  url = \"sqlite::memory:\"
}

model Widget {
  id Int @id
}
",
    );

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    assert!(package.files.is_empty());
}
