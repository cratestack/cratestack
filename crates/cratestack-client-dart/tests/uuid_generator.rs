//! Text-level proof for cratestack#1359's Dart half: what the generator
//! writes for a `Uuid`. A server writes a `Uuid` as 16 bytes under CBOR, and
//! the `cratestack_cbor` bridge (JSON text) hands it over as a list of 16
//! integers, so a decode through `as String` threw. Every decode site must go
//! through the runtime's `cratestackDecodeUuid`, which accepts the string or
//! the 16 integers, and request encoding must stay a plain string (the server
//! accepts text). No Flutter needed, so this runs everywhere.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

fn generate(fixture: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "uuid_check".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            ..DartGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("should generate under {preset:?}: {error}"))
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("missing generated file {name}"))
        .contents
        .as_str()
}

fn all_dart(package: &GeneratedDartPackage) -> String {
    package
        .files
        .iter()
        .filter(|file| file.file_name.ends_with(".dart"))
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every field and procedure shape decodes through the runtime helper, never
/// `as String`, on REST and RPC alike.
#[test]
fn uuid_decodes_through_the_runtime_helper_at_every_position() {
    for fixture in ["uuid_scalar", "uuid_scalar_rpc"] {
        let package = generate(fixture, DartPreset::Default);
        let models = file(&package, "lib/src/models.dart");

        assert!(
            models.contains("id: cratestackDecodeUuid(cratestackRequireWireValue('CreateDocInput', 'id', value['id']), 'CreateDocInput.id')"),
            "{fixture}: {models}"
        );
        assert!(
            models.contains("parentId: value['parentId'] == null ? null : cratestackDecodeUuid(value['parentId'], 'Doc.parentId')"),
            "{fixture}: {models}"
        );
        assert!(
            models.contains(".map((item) => cratestackDecodeUuid(item, 'Doc.tagIds'))"),
            "{fixture}: {models}"
        );
        assert!(
            models.contains(".map((item) => cratestackDecodeUuid(item, 'Pair.others'))"),
            "{fixture}: {models}"
        );
        assert!(
            !models.contains("value['id'] as String"),
            "{fixture}: {models}"
        );
        assert!(
            !models.contains("value['ownerId'] as String"),
            "{fixture}: {models}"
        );

        // The filter class: every operand, including the list.
        for operand in ["eq", "ne", "lt", "lte", "gt", "gte"] {
            assert!(
                models.contains(&format!(
                    "{operand}: value['{operand}'] == null ? null : cratestackDecodeUuid(value['{operand}'], 'UuidFilter.{operand}')"
                )),
                "{fixture}: UuidFilter.{operand}"
            );
        }
        assert!(
            models.contains(".map((item) => cratestackDecodeUuid(item, 'UuidFilter.in'))"),
            "{fixture}: {models}"
        );

        // A bare `Uuid` procedure return.
        let apis = file(&package, "lib/src/apis.dart");
        assert!(
            apis.contains("cratestackDecodeUuid(cratestackRequireWireValue('Procedure', 'echoId', body), 'Procedure.echoId')"),
            "{fixture}: {apis}"
        );
    }
}

/// The helper is defined once per transport runtime, and accepts a string or
/// 16 integers and nothing else.
#[test]
fn the_runtime_defines_the_helper_for_both_transports() {
    for fixture in ["uuid_scalar", "uuid_scalar_rpc"] {
        let package = generate(fixture, DartPreset::Default);
        let everything = all_dart(&package);
        assert_eq!(
            everything
                .matches("String cratestackDecodeUuid(Object? value, String site)")
                .count(),
            1,
            "{fixture}: exactly one definition"
        );
        assert!(
            everything.contains("value is List && value.length == 16"),
            "{fixture}"
        );
        assert!(
            everything.contains("expected a Uuid as a string or 16 bytes"),
            "{fixture}"
        );
    }
}

/// Requests are unchanged: a `Uuid` is a plain string on encode.
#[test]
fn uuid_requests_stay_plain_strings() {
    let package = generate("uuid_scalar", DartPreset::Default);
    let models = file(&package, "lib/src/models.dart");
    assert!(models.contains("'ownerId': ownerId,"), "{models}");
    assert!(models.contains("'parentId': parentId,"), "{models}");
    assert!(models.contains("final String ownerId;"), "{models}");
    assert!(models.contains("final String? parentId;"), "{models}");
    assert!(models.contains("final List<String> tagIds;"), "{models}");
}

/// The riverpod preset reuses the runtime and carries its own `UuidFilter`.
#[test]
fn the_riverpod_preset_decodes_uuid_through_the_helper_too() {
    for fixture in ["uuid_scalar", "uuid_scalar_rpc"] {
        let package = generate(fixture, DartPreset::Riverpod);
        let everything = all_dart(&package);
        assert!(
            everything.contains("String cratestackDecodeUuid(Object? value, String site)"),
            "{fixture}"
        );
        assert!(
            everything.contains("cratestackDecodeUuid(value['eq'], 'UuidFilter.eq')"),
            "{fixture}: the riverpod UuidFilter.fromWire"
        );
        assert!(
            everything.contains("cratestackDecodeUuid(value['id'], 'Doc.id')"),
            "{fixture}"
        );
    }
}
