//! `--native-cbor` (issue #563): gates whether the generated runtime uses
//! the published `cratestack_cbor` package (flutter_rust_bridge natively,
//! wasm-bindgen on web) instead of pure-Dart `package:cbor`. Mirrors the
//! shape of `cratestack-client-typescript`'s `--tanstack`/`--refine`/`--swr`
//! tests: a byte-identical-without-the-flag guard, a presence/shape check
//! with the flag, and an over-emission guard proving the flag only touches
//! the two files that legitimately depend on the codec choice
//! (`pubspec.yaml`, `lib/src/runtime.dart`).
//!
//! Deliberately opt-in, not the default — see
//! `DartGeneratorConfig::native_cbor`'s doc comment for why (`cratestack_cbor`
//! only supports Linux x86_64/Android/web today; defaulting to it would
//! crash every generated Flutter client on iOS). This suite pins that
//! default explicitly (`without_the_flag_...`), on top of the pre-existing
//! `tests/snapshot.rs` golden files, which never pass `native_cbor` at all
//! and so already exercise `Default::default()`.
//!
//! Structural coverage only (source-level assertions) — the real-compiler
//! proof (`dart pub get` + `flutter analyze` + a functional HTTP round trip
//! through the real `cratestack_cbor` codec, both REST and RPC, both happy
//! path and the async exception-decode path) was run by hand against a
//! generated package as part of landing this issue; see the PR description
//! for the transcript. This crate has no existing `tests/*_tsc.rs`-style
//! "shell out to the real toolchain" pattern for Dart (unlike
//! `cratestack-client-typescript`) — `just verify-dart` is this crate's
//! equivalent gate, and it runs against fixtures on disk rather than as a
//! `cargo test`.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";
const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

#[test]
fn without_the_flag_output_matches_the_default_config_exactly() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let explicit_false = generate(fixture, DartPreset::Default, false);
        let default_config = generate(fixture, DartPreset::Default, false);
        assert_eq!(
            explicit_false, default_config,
            "{fixture}: native_cbor: false must match DartGeneratorConfig::default()'s output"
        );

        let pubspec = file(&explicit_false, "pubspec.yaml");
        assert!(
            pubspec.contains("cbor: ^6.5.1"),
            "{fixture}: pubspec.yaml must still depend on package:cbor without the flag:\n{pubspec}"
        );
        assert!(
            !pubspec.contains("cratestack_cbor"),
            "{fixture}: pubspec.yaml must not mention cratestack_cbor without the flag:\n{pubspec}"
        );

        let runtime = file(&explicit_false, "lib/src/runtime.dart");
        assert!(
            runtime.contains("import 'package:cbor/simple.dart' as cbor;"),
            "{fixture}: runtime.dart must still import package:cbor without the flag:\n{runtime}"
        );
        assert!(
            !runtime.contains("cratestack_cbor"),
            "{fixture}: runtime.dart must not mention cratestack_cbor without the flag:\n{runtime}"
        );
    }
}

#[test]
fn the_flag_swaps_the_pubspec_dependency_and_the_runtime_import() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(fixture, DartPreset::Default, true);

        let pubspec = file(&package, "pubspec.yaml");
        assert!(
            pubspec.contains(&format!("cratestack_cbor: ^{}", env!("CARGO_PKG_VERSION"))),
            "{fixture}: pubspec.yaml should depend on cratestack_cbor, pinned to this crate's \
             version (lockstep with dart-packages/cratestack_cbor's own version):\n{pubspec}"
        );
        assert!(
            !pubspec.contains("cbor: ^6.5.1"),
            "{fixture}: pubspec.yaml must not also depend on package:cbor under the flag:\n{pubspec}"
        );

        let runtime = file(&package, "lib/src/runtime.dart");
        assert!(
            runtime.contains(
                "import 'package:cratestack_cbor/cratestack_cbor.dart' as cratestack_cbor;"
            ),
            "{fixture}: runtime.dart should import cratestack_cbor:\n{runtime}"
        );
        assert!(
            !runtime.contains("package:cbor/simple.dart"),
            "{fixture}: runtime.dart must not also import package:cbor under the flag:\n{runtime}"
        );
        assert!(
            runtime.contains("Future<cratestack_cbor.CratestackCborCodec> _cratestackCborCodec()"),
            "{fixture}: runtime.dart should define the cached async codec accessor:\n{runtime}"
        );
        assert!(
            runtime.contains("Future<Object?> _encodeBody(Object? body) async {"),
            "{fixture}: _encodeBody must become async under the flag (cratestack_cbor's \
             createCborCodec() is async):\n{runtime}"
        );
    }
}

#[test]
fn the_flag_is_additive_only_pubspec_and_runtime_differ() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let plain = generate(fixture, DartPreset::Default, false);
        let native = generate(fixture, DartPreset::Default, true);

        assert_eq!(
            plain.files.len(),
            native.files.len(),
            "{fixture}: --native-cbor must not add or remove files, only change contents"
        );

        for plain_file in &plain.files {
            let counterpart = native
                .files
                .iter()
                .find(|candidate| candidate.file_name == plain_file.file_name)
                .unwrap_or_else(|| {
                    panic!("{fixture}: --native-cbor dropped {}", plain_file.file_name)
                });
            if matches!(
                plain_file.file_name.as_str(),
                "pubspec.yaml" | "lib/src/runtime.dart"
            ) {
                assert_ne!(
                    plain_file.contents, counterpart.contents,
                    "{fixture}: {} was expected to differ under --native-cbor but didn't",
                    plain_file.file_name
                );
                continue;
            }
            assert_eq!(
                plain_file.contents, counterpart.contents,
                "{fixture}: --native-cbor changed {} — it must only touch pubspec.yaml and \
                 lib/src/runtime.dart",
                plain_file.file_name
            );
        }
    }
}

#[test]
fn rpc_exception_decoding_becomes_async_under_the_flag_and_every_call_site_awaits_it() {
    let plain = generate(RPC_FIXTURE, DartPreset::Default, false);
    let native = generate(RPC_FIXTURE, DartPreset::Default, true);

    let plain_runtime = file(&plain, "lib/src/runtime.dart");
    let native_runtime = file(&native, "lib/src/runtime.dart");

    assert!(
        plain_runtime.contains("CratestackRpcException _exceptionFromDio(DioException error) {"),
        "without the flag, _exceptionFromDio must stay synchronous:\n{plain_runtime}"
    );
    assert_eq!(
        plain_runtime
            .matches("throw _exceptionFromDio(error);")
            .count(),
        6,
        "without the flag, every one of the 6 call sites (3 in the JSON adapter, 3 in the CBOR \
         adapter) must call _exceptionFromDio synchronously:\n{plain_runtime}"
    );

    assert!(
        native_runtime.contains(
            "Future<CratestackRpcException> _exceptionFromDio(DioException error) async {"
        ),
        "under the flag, _exceptionFromDio must become async (it awaits the native codec to \
         decode a CBOR error body):\n{native_runtime}"
    );
    assert_eq!(
        native_runtime
            .matches("throw await _exceptionFromDio(error);")
            .count(),
        6,
        "under the flag, every one of the 6 call sites must await the now-async \
         _exceptionFromDio:\n{native_runtime}"
    );
    assert!(
        !native_runtime.contains("throw _exceptionFromDio(error);"),
        "under the flag, no call site should still call _exceptionFromDio without await:\n{native_runtime}"
    );
}

#[test]
fn riverpod_preset_pubspec_gates_the_same_way_as_the_default_preset() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let plain = generate(fixture, DartPreset::Riverpod, false);
        let native = generate(fixture, DartPreset::Riverpod, true);

        let plain_pubspec = file(&plain, "pubspec.yaml");
        assert!(plain_pubspec.contains("cbor: ^6.5.1"));
        assert!(!plain_pubspec.contains("cratestack_cbor"));

        let native_pubspec = file(&native, "pubspec.yaml");
        assert!(
            native_pubspec.contains(&format!("cratestack_cbor: ^{}", env!("CARGO_PKG_VERSION")))
        );
        assert!(!native_pubspec.contains("cbor: ^6.5.1"));

        // The riverpod preset reuses `lib/src/runtime.dart` verbatim from
        // the default preset's own generation (see `crate::riverpod`'s
        // module doc) — so it must gate exactly the same way, from the
        // same template, not a second independently-maintained copy.
        let plain_runtime = file(&plain, "lib/src/runtime.dart");
        let native_runtime = file(&native, "lib/src/runtime.dart");
        assert!(plain_runtime.contains("package:cbor/simple.dart"));
        assert!(native_runtime.contains("package:cratestack_cbor/cratestack_cbor.dart"));
    }
}

fn generate(fixture_stem: &str, preset: DartPreset, native_cbor: bool) -> GeneratedDartPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: format!("{fixture_stem}_client"),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            pb_lock: None,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor,
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: generation should succeed: {error}"))
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
