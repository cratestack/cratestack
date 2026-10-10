//! Text-level proof for ADR 0019's Dart half: what the generator writes for a
//! `BigInt`. `tests/bigint_round_trip.rs` runs the same output on the Dart VM,
//! dart2js and dart2wasm; this file needs no Flutter, so it runs everywhere
//! and pins each arm that a catch-all could swallow.
//!
//! The arms are the dangerous kind: a `BigInt` that reaches `dart_type`'s
//! `other => other` falls through to the right name by luck, the decode
//! catch-all `other.fromWire(..)` would be `BigInt.fromWire`, which does not
//! exist, and an encode that falls to `.toWire()` is the same. Each test below
//! fails if one of those arms is removed.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

fn generate(fixture: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path} should parse: {error}"));
    render(&schema, preset)
}

fn generate_from_source(source: &str, preset: DartPreset) -> GeneratedDartPackage {
    let schema = cratestack_parser::parse_schema(source)
        .unwrap_or_else(|error| panic!("inline schema should parse: {error}"));
    render(&schema, preset)
}

fn render(schema: &cratestack_core::Schema, preset: DartPreset) -> GeneratedDartPackage {
    generate_package(
        schema,
        &DartGeneratorConfig {
            library_name: "bigint_check".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
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

#[test]
fn bigint_fields_are_dart_core_bigint_decoded_and_encoded_as_strings() {
    let package = generate("bigint_scalar", DartPreset::Default);
    let models = file(&package, "lib/src/models.dart");

    // The type: `dart:core`'s `BigInt`, never `int` or `num`.
    assert!(models.contains("final BigInt? balanceE8;"), "{models}");
    assert!(models.contains("final BigInt balanceE8;"), "{models}");
    assert!(
        models.contains("final List<BigInt> perAccountE8;"),
        "{models}"
    );
    assert!(!models.contains("int? balanceE8"), "{models}");
    assert!(!models.contains("num? balanceE8"), "{models}");

    // Decode: through the runtime helper, which names the field and calls
    // `BigInt.parse`; never `(x as num).toInt()`, which rounds on dart2js.
    assert!(
        models.contains(
            "balanceE8: value['balanceE8'] == null ? null : \
             cratestackDecodeBigInt(value['balanceE8'], 'Account.balanceE8')"
        ),
        "{models}"
    );
    assert!(
        models.contains(
            "balanceE8: cratestackDecodeBigInt(cratestackRequireWireValue(\
             'CreateAccountInput', 'balanceE8', value['balanceE8']), 'CreateAccountInput.balanceE8')"
        ),
        "{models}"
    );
    assert!(
        !models.contains("balanceE8: (value['balanceE8'] as num)"),
        "{models}"
    );
    assert!(
        models.contains(".map((item) => cratestackDecodeBigInt(item, 'Report.perAccountE8'))"),
        "{models}"
    );

    // Encode: `toString()`, required and optional and list alike.
    assert!(
        models.contains("'balanceE8': balanceE8.toString()"),
        "{models}"
    );
    assert!(
        models.contains("'limitE8': limitE8?.toString()"),
        "{models}"
    );
    assert!(
        models.contains("'perAccountE8': perAccountE8.map((item) => item.toString())"),
        "{models}"
    );

    // Procedure arguments and a bare procedure return take the same arms.
    assert!(models.contains("'by': by.toString()"), "{models}");
    let apis = file(&package, "lib/src/apis.dart");
    assert!(
        apis.contains("return cratestackDecodeBigInt(cratestackRequireWireValue('Procedure', 'bump', body), 'Procedure.bump');"),
        "{apis}"
    );
    assert!(apis.contains("Future<BigInt> bump("), "{apis}");

    // `dart:core` needs no import, so a BigInt-only schema adds no dependency
    // and no import line beyond what the file already has.
    assert!(!models.contains("import 'dart:math'"), "{models}");
}

#[test]
fn bigint_has_its_own_filter_class_and_int_keeps_number_filter() {
    let package = generate("bigint_scalar", DartPreset::Default);
    let models = file(&package, "lib/src/models.dart");

    assert!(models.contains("class BigIntFilter {"), "{models}");
    for operand in ["eq", "ne", "lt", "lte", "gt", "gte"] {
        assert!(
            models.contains(&format!("final BigInt? {operand};")),
            "{operand}"
        );
    }
    assert!(models.contains("final List<BigInt>? in$;"), "{models}");
    assert!(
        models.contains("final BigIntFilter? balanceE8;"),
        "{models}"
    );
    assert!(
        models.contains("final BigIntFilter? id;"),
        "BigInt primary key filter\n{models}"
    );
    assert!(
        !models.contains("final NumberFilter? balanceE8;"),
        "{models}"
    );

    // `Int` is untouched by PR B: still `int`, still `NumberFilter`.
    let mixed = generate_from_source(
        "model Widget {\n  id Int @id\n  count Int\n  total BigInt\n  @@allow(\"list\", true)\n}\n",
        DartPreset::Default,
    );
    let models = file(&mixed, "lib/src/models.dart");
    assert!(models.contains("final int? count;"), "{models}");
    assert!(models.contains("final NumberFilter? count;"), "{models}");
    assert!(models.contains("final BigInt? total;"), "{models}");
    assert!(models.contains("final BigIntFilter? total;"), "{models}");
    assert!(
        models.contains("count: value['count'] == null ? null : (value['count'] as num).toInt()")
    );
}

#[test]
fn both_runtimes_define_the_canonical_bigint_decoder() {
    for fixture in ["bigint_scalar", "bigint_scalar_rpc"] {
        let package = generate(fixture, DartPreset::Default);
        let runtime = file(&package, "lib/src/runtime.dart");
        let signature = "BigInt cratestackDecodeBigInt(Object? value, String site) {";
        let start = runtime
            .find(signature)
            .unwrap_or_else(|| panic!("{fixture}: no cratestackDecodeBigInt in\n{runtime}"));
        let body =
            &runtime[start..start + runtime[start..].find("\n}\n").expect("end of function")];
        // A non-string is refused outright; a string is checked against the
        // canonical grammar and only then parsed, with `BigInt.parse`.
        assert!(
            body.contains("if (value is! String) {"),
            "{fixture}: {body}"
        );
        assert!(
            body.contains("_cratestackCanonicalBigInt.hasMatch(value)"),
            "{fixture}: {body}"
        );
        assert!(
            body.contains("return BigInt.parse(value);"),
            "{fixture}: {body}"
        );
        assert!(
            runtime.contains(r"RegExp(r'^(0|-?[1-9][0-9]*)$')"),
            "{fixture}: the canonical grammar changed"
        );
        // The decoder never takes the lossy road.
        assert!(!body.contains("int.parse"), "{fixture}: {body}");
        assert!(!body.contains("BigInt.from("), "{fixture}: {body}");
        assert!(!body.contains("toInt()"), "{fixture}: {body}");
    }
}

#[test]
fn the_rest_client_interpolates_the_key_and_the_rpc_client_sends_a_string() {
    let rest = generate("bigint_scalar", DartPreset::Default);
    let apis = file(&rest, "lib/src/apis.dart");
    // `$id` in a path calls `toString()`, which is canonical for a BigInt.
    assert!(apis.contains("Future<Ledger> get(BigInt id, {"), "{apis}");
    assert!(apis.contains("'/ledgers/$id'"), "{apis}");

    let rpc = generate("bigint_scalar_rpc", DartPreset::Default);
    let apis = file(&rpc, "lib/src/apis.dart");
    // The frame holds a string for the BigInt key, and the raw value for a
    // key that is already a JSON value (the Cuid-keyed `Account`).
    assert!(apis.contains("{'id': id.toString()},"), "{apis}");
    assert!(
        apis.contains("{'id': id.toString(), 'patch': patch.toWire()},"),
        "{apis}"
    );
    assert!(apis.contains("{'id': id},"), "Cuid key stays as is\n{apis}");
    assert!(
        apis.contains("{'id': id, 'patch': patch.toWire()},"),
        "{apis}"
    );
}

#[test]
fn the_riverpod_preset_gets_a_mappable_bigint_filter_and_a_string_rpc_key() {
    let rest = generate("bigint_scalar", DartPreset::Riverpod);
    let shared = file(&rest, "lib/src/models/shared_types.dart");
    // `generateMethods: equals | copy` only, so dart_mappable never needs a
    // `BigInt` mapper; equality of two operands falls back to `BigInt.==`.
    assert!(
        shared.contains(
            "@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)\n\
             class BigIntFilter with BigIntFilterMappable {"
        ),
        "{shared}"
    );
    assert!(shared.contains("final BigInt? eq;"), "{shared}");
    assert!(
        shared.contains("cratestackDecodeBigInt(value['eq'], 'BigIntFilter.eq')"),
        "{shared}"
    );
    assert!(shared.contains("'eq': eq?.toString(),"), "{shared}");

    let account = file(&rest, "lib/src/models/account.dart");
    assert!(
        account.contains("final BigIntFilter? balanceE8;"),
        "{account}"
    );
    assert!(account.contains("final BigInt? balanceE8;"), "{account}");
    // BigInt is `dart:core`: no scalar import appears for it.
    assert!(!account.contains("package:decimal"), "{account}");
    assert!(!account.contains("dart:typed_data"), "{account}");

    let rpc = generate("bigint_scalar_rpc", DartPreset::Riverpod);
    let ledger = file(&rpc, "lib/src/models/ledger.dart");
    assert!(ledger.contains("{'id': id.toString()}"), "{ledger}");
    assert!(
        ledger.contains("{'id': id.toString(), 'patch': patch.toWire()}"),
        "{ledger}"
    );
}

#[test]
fn the_bigint_readme_section_ships_in_both_presets() {
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let package = generate("bigint_scalar", preset);
        let readme = file(&package, "README.md");
        assert!(readme.contains("## BigInt Fields"), "{preset:?}");
        assert!(readme.contains("canonical decimal string"), "{preset:?}");
    }
}
