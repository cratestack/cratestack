//! Compact wire keys in the generated Dart package
//! (`docs/design/compact-wire-keys.md`): structural coverage. The behavioural
//! proof (negotiation, transcoding, the encoding a request goes out in) runs
//! under `just verify-dart` from `tests/fixtures/compact_hooks_{rest,rpc}_test.dart`.

use base64::Engine as _;
use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

fn generate(fixture: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: format!("{fixture}_client"),
            preset,
            ..DartGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture}: generation should succeed: {error}"))
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
        .contents
        .as_str()
}

#[test]
fn key_tables_constant_is_the_core_artifact_for_every_package() {
    for fixture in ["tiny_rest", "tiny_rpc"] {
        let schema =
            cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
                .expect("parses");
        let expected = base64::engine::general_purpose::STANDARD
            .encode(cratestack_core::compact::KeyTables::from_schema(&schema).to_bytes());
        for preset in [
            DartPreset::Default,
            DartPreset::Riverpod,
            DartPreset::Fespalier,
        ] {
            let constants = generate(fixture, preset);
            let constants = file(&constants, "lib/src/constants.dart");
            assert!(
                constants.contains("final Uint8List cratestackKeyTables = base64Decode("),
                "{fixture}/{preset:?}: cratestackKeyTables is emitted"
            );
            assert!(
                constants.contains(&format!("'{expected}'")),
                "{fixture}/{preset:?}: the constant is KeyTables::from_schema(..).to_bytes()"
            );
        }
    }
}

#[test]
fn both_adapters_take_hooks_and_default_to_the_unchanged_wire() {
    for (fixture, adapter) in [
        ("tiny_rest", "CratestackHttpAdapter"),
        ("tiny_rpc", "CratestackRpcHttpAdapter"),
    ] {
        let package = generate(fixture, DartPreset::Default);
        let runtime = file(&package, "lib/src/runtime.dart");
        assert!(
            runtime.contains("class CratestackCompactHooks {"),
            "{fixture}"
        );
        assert!(
            runtime.contains("final CratestackCompactHooks? compact;"),
            "{fixture}: {adapter} has an optional `compact` field"
        );
        assert!(
            runtime.contains("this.compact,"),
            "{fixture}: optional ctor param"
        );
        assert!(
            runtime.contains("if (hooks == null || format != CratestackWireFormat.cbor) {"),
            "{fixture}: no hooks and JSON never go compact"
        );
    }
}

#[test]
fn rest_names_the_op_by_route_template_and_rpc_by_op_id() {
    let rest = generate("tiny_rest", DartPreset::Default);
    let rest = file(&rest, "lib/src/runtime.dart");
    assert!(rest.contains("String? _cratestackRestOp(String method, String path) {"));
    assert!(rest.contains("for (final op in cratestackOpContracts.keys)"));
    let rpc = generate("tiny_rpc", DartPreset::Default);
    let rpc = file(&rpc, "lib/src/runtime.dart");
    assert!(
        !rpc.contains("_cratestackRestOp"),
        "RPC has no route templates"
    );
    assert!(rpc.contains(
        "_cratestackCompactBegin(\n      this,\n      compact,\n      format,\n      opId,"
    ));
}

#[test]
fn batch_frames_are_never_compact() {
    let rpc = generate("tiny_rpc", DartPreset::Default);
    let runtime = file(&rpc, "lib/src/runtime.dart");
    let adapter = runtime
        .find("class CratestackRpcHttpAdapter")
        .expect("adapter");
    let start = adapter
        + runtime[adapter..]
            .find("Future<List<RpcResponseFrame>> batch(")
            .expect("batch");
    let end = runtime[start..]
        .find("@override")
        .map_or(runtime.len(), |at| start + at);
    let batch = &runtime[start..end];
    assert!(
        !batch.contains("compact"),
        "batch passes no compact plan:\n{batch}"
    );
}

#[test]
fn streams_read_compact_sequences_and_keep_the_error_item_named() {
    let rpc = generate("tiny_rpc", DartPreset::Default);
    let runtime = file(&rpc, "lib/src/runtime.dart");
    assert!(runtime.contains("application/vnd.cratestack.compact+cbor-seq"));
    let tag_check = runtime
        .find("cratestackRpcStreamErrorTag) {")
        .expect("error tag check");
    let transcode = runtime
        .find("compactItems ? plan.decode(item)")
        .expect("item transcode");
    assert!(
        tag_check < transcode,
        "the error sentinel is checked before an item is transcoded"
    );
}

#[test]
fn the_fespalier_adapter_provider_reads_a_compact_provider() {
    for fixture in ["tiny_rest", "tiny_rpc"] {
        let package = generate(fixture, DartPreset::Fespalier);
        let client = file(&package, "lib/src/client.dart");
        assert!(
            client.contains("Provider<CratestackCompactHooks?>((ref) => null);"),
            "{fixture}: CompactProvider defaults to null"
        );
        assert!(
            client.contains("compact: ref.watch("),
            "{fixture}: the default adapter reads it"
        );
    }
    // Only the preset that builds the adapter itself grows the provider.
    let package = generate("tiny_rest", DartPreset::Riverpod);
    assert!(!file(&package, "lib/src/client.dart").contains("CompactProvider"));
}
