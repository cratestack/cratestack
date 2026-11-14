//! cratestack#1303: the device-auth marker is for `generate-dart --preset
//! fespalier`. The WireMock stubs for a schema are byte-for-byte the same
//! with the markers as without.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};

const MARKED: &str = include_str!("fixtures/device_auth_rpc.cstack");

fn stubs(source: &str) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema_unvalidated(source).expect("fixture parses");
    generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generates")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn the_stubs_are_the_same_with_and_without_the_markers() {
    let plain = MARKED
        .lines()
        .filter(|line| !line.trim_start().starts_with("@device_auth"))
        .collect::<Vec<_>>()
        .join("\n");
    let marked = stubs(MARKED);
    assert_eq!(marked.len(), 6, "one stub per procedure");
    assert_eq!(marked, stubs(&plain));
}
