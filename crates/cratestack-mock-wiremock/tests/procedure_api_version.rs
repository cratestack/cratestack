//! `@api_version` regression guard: a stub for a versioned procedure must
//! match the path the server mounts and the generated clients call,
//! `/<version>/$procs/<name>`. Before the fix `mapping.rs` hardcoded
//! `/$procs/<name>`, so once the clients called the versioned path these
//! stubs would never have matched them. The path now comes from
//! `cratestack_core::procedure_route`, the function the server mounts with.
//! `transport rpc` is unaffected: its op id is `procedure.<name>` on both
//! sides, version or not.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};

fn url_paths(source: &str) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("schema should parse");
    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generation should succeed");
    let mut paths: Vec<(String, String)> = package
        .files
        .iter()
        .map(|file| {
            let mapping: serde_json::Value = serde_json::from_str(&file.contents).unwrap();
            (
                file.file_name.clone(),
                mapping["request"]["urlPath"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    paths.sort();
    paths
}

const PROCEDURES: &str = "
type Greeting {
  message String
}

procedure hello(): Greeting
  @api_version(\"v2\")

procedure plain(): Greeting
";

#[test]
fn rest_stub_for_a_versioned_procedure_matches_the_mounted_path() {
    let source = format!("datasource db {{\n  provider = \"none\"\n}}\n{PROCEDURES}");
    assert_eq!(
        url_paths(&source),
        [
            (
                "mappings/hello.json".to_owned(),
                "/api/v2/$procs/hello".to_owned()
            ),
            (
                "mappings/plain.json".to_owned(),
                "/api/$procs/plain".to_owned()
            ),
        ]
    );
}

#[test]
fn rpc_stub_for_a_versioned_procedure_keeps_the_unversioned_op_id() {
    let source =
        format!("transport rpc\n\ndatasource db {{\n  provider = \"none\"\n}}\n{PROCEDURES}");
    assert_eq!(
        url_paths(&source),
        [
            (
                "mappings/hello.json".to_owned(),
                "/api/rpc/procedure.hello".to_owned()
            ),
            (
                "mappings/plain.json".to_owned(),
                "/api/rpc/procedure.plain".to_owned()
            ),
        ]
    );
}
