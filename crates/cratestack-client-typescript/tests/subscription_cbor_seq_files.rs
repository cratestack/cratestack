//! The `"cbor-seq"` subscription transport's files (cratestack#1001): every
//! schema with a subscription gets `src/subscription-cbor-seq.ts`, a REST one
//! also the structural CBOR walker it frames items with (an RPC package has it
//! already, for `@stream`), and a schema with none gets nothing new.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const REST_SUBSCRIBING: &str = r#"
model Note {
  id Int @id
  body String

  @@allow("read", true)
  @@emit(created)
  @@subscribe
}
"#;

const RPC_SUBSCRIBING: &str = "transport rpc\n\nmodel Note {\n  id Int @id\n  body String\n\n  @@allow(\"read\", true)\n  @@emit(created)\n  @@subscribe\n}\n";

const REST_QUIET: &str = "model Note {\n  id Int @id\n  body String\n}\n";

fn files(source: &str) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture should parse");
    generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("package should render")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

fn names(files: &[(String, String)]) -> Vec<&str> {
    files.iter().map(|(name, _)| name.as_str()).collect()
}

fn contents<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(file, _)| file == name)
        .unwrap_or_else(|| panic!("{name} was not generated"))
        .1
        .as_str()
}

#[test]
fn a_rest_package_with_subscriptions_gets_the_binding_and_the_walker_it_needs() {
    let files = files(REST_SUBSCRIBING);
    let names = names(&files);
    for name in [
        "src/subscription-cbor-seq.ts",
        "src/cbor-item.ts",
        "src/cbor-argument.ts",
    ] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "a file is emitted twice: {names:?}"
    );
    let subscriptions = contents(&files, "src/subscriptions.ts");
    assert!(subscriptions.contains(r#""sse" | "cbor-seq""#));
    assert!(subscriptions.contains("openCborSeq"));
}

#[test]
fn an_rpc_package_shares_the_walker_it_already_has() {
    let files = files(RPC_SUBSCRIBING);
    let names = names(&files);
    assert!(names.contains(&"src/subscription-cbor-seq.ts"));
    for shared in [
        "src/cbor-item.ts",
        "src/cbor-argument.ts",
        "src/cbor-seq.ts",
    ] {
        assert_eq!(
            names.iter().filter(|name| **name == shared).count(),
            1,
            "{shared} must be emitted once: {names:?}"
        );
    }
}

#[test]
fn a_schema_without_subscriptions_renders_none_of_it() {
    let files = files(REST_QUIET);
    let names = names(&files);
    assert!(
        !names.iter().any(|name| name.contains("subscription")),
        "{names:?}"
    );
    assert!(!names.contains(&"src/cbor-item.ts"), "{names:?}");
}

#[test]
fn the_binding_asks_for_the_media_type_and_reuses_the_walker() {
    let files = files(REST_SUBSCRIBING);
    let binding = contents(&files, "src/subscription-cbor-seq.ts");
    assert!(binding.contains(r#"SUBSCRIPTION_CBOR_SEQ_CONTENT_TYPE = "application/cbor-seq""#));
    assert!(binding.contains(r#"from "./cbor-item.js""#));
    assert!(binding.contains("CratestackEventTooLargeError"));
}
