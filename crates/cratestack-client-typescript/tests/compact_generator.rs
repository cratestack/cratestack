//! The generated TypeScript packages carry the schema's compact wire key
//! tables and the opt-in `compactKeys` option; the default wire (option off)
//! is pinned byte for byte by `tests/snapshot.rs`.

use base64::Engine as _;
use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};
use cratestack_core::compact::KeyTables;

const REST: &str =
    "model Widget {\n  id Int @id\n  name String\n  @@allow(\"read\", auth() != null)\n}\n";

fn package(source: &str, swr: bool) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("parses");
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            swr,
            ..Default::default()
        },
    )
    .expect("renders")
    .files
    .into_iter()
    .map(|file| (file.file_name, file.contents))
    .collect()
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    &files
        .iter()
        .find(|(file_name, _)| file_name == name)
        .unwrap_or_else(|| panic!("{name} is generated"))
        .1
}

fn key_tables_constant(runtime: &str) -> Vec<u8> {
    let marker = "export const KEY_TABLES: string = \"";
    let start = runtime.find(marker).expect("KEY_TABLES is emitted") + marker.len();
    let end = start + runtime[start..].find('"').expect("closing quote");
    base64::engine::general_purpose::STANDARD
        .decode(&runtime[start..end])
        .expect("KEY_TABLES is base64")
}

#[test]
fn key_tables_are_the_cores_artifact_in_every_runtime() {
    for (source, swr) in [
        (REST.to_owned(), false),
        (format!("transport rpc\n{REST}"), false),
        (format!("transport rpc\n{REST}"), true),
    ] {
        let schema = cratestack_parser::parse_schema(&source).expect("parses");
        let expected = KeyTables::from_schema(&schema).to_bytes();
        let files = package(&source, swr);
        let mut runtimes = vec!["src/runtime.ts"];
        if swr {
            runtimes.push("src/swr/runtime.ts");
        }
        for runtime in runtimes {
            assert_eq!(
                key_tables_constant(file(&files, runtime)),
                expected,
                "{runtime} swr={swr}"
            );
        }
    }
}

#[test]
fn compact_keys_is_an_opt_in_option_of_both_runtimes() {
    let rpc = package(&format!("transport rpc\n{REST}"), false);
    let runtime = file(&rpc, "src/runtime.ts");
    assert!(runtime.contains("compactKeys?: boolean;"));
    assert!(runtime.contains("options.compactKeys === true"));
    let compact = file(&rpc, "src/compact.ts");
    for needle in [
        "application/vnd.cratestack.compact+cbor\"",
        "application/vnd.cratestack.compact+json\"",
        "application/vnd.cratestack.compact+cbor-seq\"",
        "Cratestack-Contract",
        "createCompactKeys",
    ] {
        assert!(compact.contains(needle), "rpc compact.ts lacks {needle}");
    }

    let rest = package(REST, false);
    assert!(file(&rest, "src/runtime.ts").contains("compactKeys?: boolean;"));
    assert!(file(&rest, "src/compact.ts").contains("import(\"@cratestack/cbor\")"));
}

#[test]
fn rest_declares_cbor_as_an_optional_peer_and_rpc_does_not() {
    let rest = package(REST, false);
    let manifest = file(&rest, "package.json");
    let json: serde_json::Value = serde_json::from_str(manifest).expect("package.json parses");
    assert!(json["peerDependencies"]["@cratestack/cbor"].is_string());
    assert_eq!(
        json["peerDependenciesMeta"]["@cratestack/cbor"]["optional"],
        true
    );
    assert!(json["dependencies"]["@cratestack/cbor"].is_null());

    let rpc = package(&format!("transport rpc\n{REST}"), false);
    let json: serde_json::Value =
        serde_json::from_str(file(&rpc, "package.json")).expect("package.json parses");
    assert!(json["dependencies"]["@cratestack/cbor"].is_string());
    assert!(json["peerDependencies"]["@cratestack/cbor"].is_null());
    assert!(json.get("peerDependenciesMeta").is_none());
}
