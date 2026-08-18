// Golden-file snapshot tests for the Dart generator. Mirrors the
// TypeScript generator's snapshot suite — same two fixtures, same
// `CRATESTACK_UPDATE_SNAPSHOTS=1` opt-in for regenerating.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

/// Deterministic stand-in for `cli_support::hash_schema_source`'s real
/// output (issue #178) — a real SHA-256 hex digest, distinct from
/// `tests/generator.rs`'s own constant, purely so the snapshot fixtures
/// carry a value obviously not copy-pasted from elsewhere.
const SNAPSHOT_SCHEMA_SHA256: &str =
    "9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4";

#[test]
fn rest_snapshot_matches_fixture() {
    run_snapshot("tiny_rest", "tiny_rest_client");
}

#[test]
fn rpc_snapshot_matches_fixture() {
    run_snapshot("tiny_rpc", "tiny_rpc_client");
}

#[test]
fn rpc_apis_invoke_adapter_call_with_dotted_op_ids() {
    let package = generate_for("tiny_rpc", "tiny_rpc_client");
    let apis = package_file(&package, "lib/src/apis.dart");
    for op in [
        "model.Widget.list",
        "model.Widget.get",
        "model.Widget.create",
        "model.Widget.update",
        "model.Widget.delete",
        "procedure.echoName",
    ] {
        assert!(
            apis.contains(&format!("'{op}'")),
            "apis.dart is missing the `{op}` op id:\n{apis}"
        );
    }
    assert!(
        apis.contains("CratestackRpcAdapter"),
        "apis.dart must take a CratestackRpcAdapter, not the REST adapter"
    );
}

#[test]
fn rpc_runtime_defines_adapter_interface_and_exception() {
    let package = generate_for("tiny_rpc", "tiny_rpc_client");
    let runtime = package_file(&package, "lib/src/runtime.dart");
    assert!(
        runtime.contains("abstract interface class CratestackRpcAdapter"),
        "runtime.dart must declare the CratestackRpcAdapter interface"
    );
    assert!(
        runtime.contains("class CratestackRpcDioAdapter"),
        "runtime.dart must ship the JSON adapter"
    );
    assert!(
        runtime.contains("class CratestackRpcCborDioAdapter"),
        "runtime.dart must ship the CBOR adapter"
    );
    assert!(
        runtime.contains("class CratestackRpcException"),
        "runtime.dart must declare the CratestackRpcException error"
    );
    assert!(
        runtime.contains("'not_found'"),
        "runtime.dart must surface the `not_found` gRPC-style code"
    );
}

#[test]
fn rest_runtime_keeps_existing_adapter_interface() {
    let package = generate_for("tiny_rest", "tiny_rest_client");
    let runtime = package_file(&package, "lib/src/runtime.dart");
    assert!(
        runtime.contains("abstract interface class CratestackClientAdapter"),
        "REST runtime.dart must keep CratestackClientAdapter"
    );
    let apis = package_file(&package, "lib/src/apis.dart");
    assert!(
        !apis.contains("'/rpc/"),
        "REST apis.dart should not reference /rpc/ URLs"
    );
}

/// Regression test for a real bug found while building the
/// `examples/flutter-riverpod` end-to-end example (issue #303):
/// `CratestackDioAdapter.execute` never set an `Accept` header, unlike
/// `CratestackCborDioAdapter` right below it (which always has) and the
/// TypeScript REST runtime's `headers.set("Accept", "application/json")`
/// (`crates/cratestack-client-typescript/templates/rest-runtime.ts.j2`).
/// Reproduced live: a plain `curl` with no `Accept` header against a
/// real `--preset riverpod` example server (Postgres + axum, wired with
/// only `JsonCodec`) returned "no encoder configured for response
/// Content-Type application/cbor", not JSON — the router's own
/// content-negotiation default doesn't fall back to the one codec that
/// was actually registered. Every REST JSON adapter call must state its
/// preference explicitly rather than rely on that default.
#[test]
fn rest_dio_adapter_sends_an_explicit_json_accept_header() {
    let package = generate_for("tiny_rest", "tiny_rest_client");
    let runtime = package_file(&package, "lib/src/runtime.dart");
    let dio_adapter_start = runtime
        .find("class CratestackDioAdapter")
        .expect("runtime.dart should declare CratestackDioAdapter");
    let cbor_adapter_start = runtime
        .find("class CratestackCborDioAdapter")
        .expect("runtime.dart should declare CratestackCborDioAdapter");
    let dio_adapter_body = &runtime[dio_adapter_start..cbor_adapter_start];
    assert!(
        dio_adapter_body.contains("'Accept': 'application/json'"),
        "CratestackDioAdapter must send an explicit Accept: application/json \
         header, matching CratestackCborDioAdapter's own explicit Accept and \
         the TypeScript REST runtime's parity behaviour:\n{dio_adapter_body}"
    );
}

#[test]
fn rpc_skips_queries_dart() {
    let rpc = generate_for("tiny_rpc", "tiny_rpc_client");
    assert!(
        rpc.files
            .iter()
            .all(|file| file.file_name != "lib/src/queries.dart"),
        "RPC schemas should not emit queries.dart"
    );
    let rest = generate_for("tiny_rest", "tiny_rest_client");
    assert!(
        rest.files
            .iter()
            .any(|file| file.file_name == "lib/src/queries.dart"),
        "REST schemas should still emit queries.dart"
    );
}

/// Regression test for a real bug: `example/main.dart` and
/// `test/*_test.dart` used to be shared, transport-agnostic templates
/// that hard-coded `CratestackFetchQuery`/`{Model}Selection` — types only
/// `rest-queries.dart.j2` (REST-only) defines. Every RPC-transport
/// generated package shipped an example and a test file that failed
/// `flutter analyze` with undefined-symbol errors. Confirmed by
/// generating `tiny_rpc.cstack` and running `flutter analyze` on the
/// output before this fix landed. RPC now gets its own
/// `rpc-example-main.dart.j2`/`rpc-package-test.dart.j2` pair.
#[test]
fn rpc_example_and_test_do_not_reference_rest_only_query_types() {
    let rpc = generate_for("tiny_rpc", "tiny_rpc_client");
    let example = package_file(&rpc, "example/main.dart");
    let test_file = package_file(&rpc, "test/tiny_rpc_client_test.dart");

    for content in [example, test_file] {
        assert!(
            !content.contains("CratestackFetchQuery") && !content.contains("Selection"),
            "RPC example/test must not reference REST-only query-builder types:\n{content}"
        );
        assert!(
            content.contains("CratestackRpcCallOptions"),
            "RPC example/test should exercise the RPC transport's own runtime type:\n{content}"
        );
        // `Widget` is `tiny_rpc.cstack`'s sole model — its projection
        // class round-trips through fromWire/toWire with no required
        // constructor args (every field is optional on that class kind),
        // so this is safe to assert generically.
        assert!(
            content.contains("Widget.fromWire(const <String, Object?>{})"),
            "RPC example/test should round-trip the generated model class:\n{content}"
        );
    }
}

fn run_snapshot(fixture_stem: &str, library_name: &str) {
    let package = generate_for(fixture_stem, library_name);
    let snapshot_dir = snapshot_root().join(fixture_stem);
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn generate_for(fixture_stem: &str, library_name: &str) -> GeneratedDartPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            pb_lock: None,
            schema_sha256: SNAPSHOT_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("default template should render")
}

fn write_snapshot(dir: &Path, package: &GeneratedDartPackage) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedDartPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-dart` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn package_file<'a>(package: &'a GeneratedDartPackage, file_name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
