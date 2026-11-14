//! cratestack#1303: the device-auth marker is for `generate-dart --preset
//! fespalier`. The TypeScript generator's output for a schema is byte-for-byte
//! the same with the markers as without, on both transports and with `--swr`.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

fn without_markers(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("@device_auth"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn generate(source: &str, swr: bool) -> Vec<(String, String)> {
    // Unvalidated: the unmarked copy of a schema is a different, valid one,
    // and the marked one is checked by the parser's own tests.
    let schema = cratestack_parser::parse_schema_unvalidated(source).expect("fixture parses");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "device-auth-client".to_owned(),
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("generates");
    package
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn the_typescript_output_is_the_same_with_and_without_the_markers() {
    for source in [
        include_str!("fixtures/device_auth_rpc.cstack"),
        include_str!("fixtures/device_auth_rest.cstack"),
    ] {
        assert!(source.contains("@device_auth("));
        for swr in [false, true] {
            let marked = generate(source, swr);
            assert!(!marked.is_empty());
            assert_eq!(marked, generate(&without_markers(source), swr), "swr={swr}");
        }
    }
}
