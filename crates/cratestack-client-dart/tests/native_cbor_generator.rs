//! `native_cbor` (issue #563): gates whether the generated runtime uses
//! the published `cratestack_cbor` package (flutter_rust_bridge natively,
//! wasm-bindgen on web) instead of pure-Dart `package:cbor`. Mirrors the
//! shape of `cratestack-client-typescript`'s `--tanstack`/`--refine`/`--swr`
//! tests: a genuine reads-the-real-default guard, a presence/shape check
//! with the flag off (CLI: `--no-native-cbor`), and an over-emission guard
//! proving the flag only touches the two files that legitimately depend on
//! the codec choice (`pubspec.yaml`, `lib/src/runtime.dart`).
//!
//! **Native is now the default** (`DartGeneratorConfig::DEFAULT_NATIVE_CBOR`
//! is `true` — see its doc comment for the history). The original reason it
//! was opt-in (`cratestack_cbor` supported only Linux x86_64/Android/web, so
//! defaulting would crash generated clients on iOS) and the reason it
//! stayed opt-in after that (the published package lagging the repo) are
//! both closed: cratestack#563 landed Windows, macOS and iOS support, and
//! `cratestack_cbor` 0.8.7 carrying that matrix is published on pub.dev.
//! Only Linux arm64 remains unsupported, reached via `--no-native-cbor`.
//! `default_config_uses_native_cbor` below reads `DartGeneratorConfig::
//! default()` directly (not a hardcoded bool) so it fails if the constant
//! is ever flipped back without updating this test.
//!
//! Structural coverage only (source-level assertions) — the real-compiler
//! proof (`flutter pub get` + `flutter analyze` + a functional HTTP round
//! trip through the real `cratestack_cbor` codec, both REST and RPC, both
//! happy path and the async exception-decode path) originally ran only
//! once by hand against a generated package while landing this issue
//! (cratestack#647); `just verify-dart` now re-runs that proof on every CI
//! run, from the `native_cbor_echo{,_rpc}.cstack` and
//! `native_cbor_echo_{rest,rpc}_test.dart` fixtures — see that recipe's own
//! comment. This crate has no existing `tests/*_tsc.rs`-style "shell out to
//! the real toolchain" `cargo test` pattern for Dart (unlike
//! `cratestack-client-typescript`); `just verify-dart` is this crate's
//! equivalent gate, wired into the `dart-verify` CI job, and it runs
//! against fixtures on disk rather than as a `cargo test`.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";

/// cratestack#779: the `cratestack_cbor` API floor a generated pubspec
/// declares, restated here as a **literal** rather than recomputed from
/// `env!("CARGO_PKG_VERSION")` the way this file did before.
///
/// That is the entire point, not an oversight. The old assertion derived
/// its expected value from the same input the generator derived *its*
/// value from, so it agreed with the generator by construction and could
/// not observe the defect #779 is about — it passed just as happily when
/// the emitted pin tracked the release version. A literal disagrees the
/// moment the generator starts moving with `just bump` again: at the next
/// version this test still expects `^0.8.0`, and a regressed generator
/// emitting `^0.9.0` fails here rather than at a user's `pub get`.
///
/// Kept deliberately in sync by hand with
/// `cratestack_client_dart::package_floors::CRATESTACK_CBOR_FLOOR`
/// (`pub(crate)`, so not callable from an integration test). Raising the
/// floor there is supposed to require touching this line too.
const CRATESTACK_CBOR_FLOOR: &str = "^0.9.3";
const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// The check this replaces (`without_the_flag_output_matches_the_default_config_exactly`)
/// compared `generate(fixture, DartPreset::Default, false)` against itself —
/// both arguments hardcoded `false` — so it never actually constructed
/// `DartGeneratorConfig::default()` and would pass regardless of what
/// `DEFAULT_NATIVE_CBOR` was set to. This version reads the real default by
/// constructing `DartGeneratorConfig::default()` and asserting on its
/// `native_cbor` field and its generated output directly, so flipping
/// `DEFAULT_NATIVE_CBOR` back to `false` fails this test.
#[test]
fn default_config_uses_native_cbor() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let fixture_path = format!("tests/fixtures/{fixture}.cstack");
        let schema = cratestack_parser::parse_schema_file(&fixture_path)
            .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));

        let config = DartGeneratorConfig::default();
        assert!(
            config.native_cbor,
            "{fixture}: DartGeneratorConfig::default().native_cbor must be true \
             (DEFAULT_NATIVE_CBOR) now that cratestack_cbor 0.8.7 covers every platform but \
             Linux arm64"
        );

        let package = generate_package(&schema, &config)
            .unwrap_or_else(|error| panic!("{fixture}: generation should succeed: {error}"));

        let pubspec = file(&package, "pubspec.yaml");
        assert!(
            pubspec.contains(&format!("cratestack_cbor: {CRATESTACK_CBOR_FLOOR}")),
            "{fixture}: DartGeneratorConfig::default()'s pubspec.yaml must depend on \
             cratestack_cbor by default:\n{pubspec}"
        );
        assert!(
            !pubspec.contains("cbor: ^6.5.1"),
            "{fixture}: DartGeneratorConfig::default()'s pubspec.yaml must NOT depend on \
             package:cbor by default:\n{pubspec}"
        );

        let runtime = file(&package, "lib/src/runtime.dart");
        assert!(
            runtime.contains(
                "import 'package:cratestack_cbor/cratestack_cbor.dart' as cratestack_cbor;"
            ),
            "{fixture}: DartGeneratorConfig::default()'s runtime.dart must import \
             cratestack_cbor:\n{runtime}"
        );
        assert!(
            !runtime.contains("package:cbor/simple.dart"),
            "{fixture}: DartGeneratorConfig::default()'s runtime.dart must NOT import \
             package:cbor:\n{runtime}"
        );
    }
}

#[test]
fn no_native_cbor_falls_back_to_package_cbor() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let plain = generate(fixture, DartPreset::Default, false);

        let pubspec = file(&plain, "pubspec.yaml");
        assert!(
            pubspec.contains("cbor: ^6.5.1"),
            "{fixture}: native_cbor: false pubspec.yaml must depend on package:cbor:\n{pubspec}"
        );
        assert!(
            !pubspec.contains("cratestack_cbor"),
            "{fixture}: native_cbor: false pubspec.yaml must not mention cratestack_cbor:\n{pubspec}"
        );

        let runtime = file(&plain, "lib/src/runtime.dart");
        assert!(
            runtime.contains("import 'package:cbor/simple.dart' as cbor;"),
            "{fixture}: native_cbor: false runtime.dart must import package:cbor:\n{runtime}"
        );
        assert!(
            !runtime.contains("cratestack_cbor"),
            "{fixture}: native_cbor: false runtime.dart must not mention cratestack_cbor:\n{runtime}"
        );
    }
}

#[test]
fn the_flag_swaps_the_pubspec_dependency_and_the_runtime_import() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(fixture, DartPreset::Default, true);

        let pubspec = file(&package, "pubspec.yaml");
        assert!(
            pubspec.contains(&format!("cratestack_cbor: {CRATESTACK_CBOR_FLOOR}")),
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
            "{fixture}: native_cbor must not add or remove files, only change contents"
        );

        for plain_file in &plain.files {
            let counterpart = native
                .files
                .iter()
                .find(|candidate| candidate.file_name == plain_file.file_name)
                .unwrap_or_else(|| {
                    panic!(
                        "{fixture}: native_cbor: true dropped {}",
                        plain_file.file_name
                    )
                });
            if matches!(
                plain_file.file_name.as_str(),
                "pubspec.yaml" | "lib/src/runtime.dart"
            ) {
                assert_ne!(
                    plain_file.contents, counterpart.contents,
                    "{fixture}: {} was expected to differ under native_cbor: true but didn't",
                    plain_file.file_name
                );
                continue;
            }
            assert_eq!(
                plain_file.contents, counterpart.contents,
                "{fixture}: native_cbor: true changed {} — it must only touch pubspec.yaml and \
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
        assert!(native_pubspec.contains(&format!("cratestack_cbor: {CRATESTACK_CBOR_FLOOR}")));
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
