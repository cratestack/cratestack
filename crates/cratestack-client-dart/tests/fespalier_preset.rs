//! cratestack#1204: `--preset fespalier` is the `riverpod` preset sending
//! through fespalier's `HttpTransport` instead of Dio.
//!
//! Structural coverage (source-level). The behavioural proof (golden wire
//! parity, the exact CBOR bytes, abort through `TransportZone`, failure
//! mapping, the providers) runs under `just verify-dart-fespalier` from
//! `tests/fixtures/fespalier_transport_{rest,rpc}_test.dart`.

use std::collections::BTreeMap;

use cratestack_client_dart::{
    DEFAULT_FESPALIER_REF, DartGeneratorConfig, DartGeneratorError, DartPreset,
    GeneratedDartPackage, generate_package,
};

const FIXTURES: [&str; 2] = ["tiny_rest", "tiny_rpc"];

fn try_generate(
    fixture: &str,
    preset: DartPreset,
    native_cbor: bool,
    fespalier_ref: &str,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: format!("{fixture}_client"),
            preset,
            native_cbor,
            fespalier_ref: fespalier_ref.to_owned(),
            ..DartGeneratorConfig::default()
        },
    )
}

fn generate(fixture: &str, preset: DartPreset, native_cbor: bool) -> GeneratedDartPackage {
    try_generate(fixture, preset, native_cbor, DEFAULT_FESPALIER_REF)
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

fn has_file(package: &GeneratedDartPackage, name: &str) -> bool {
    package.files.iter().any(|file| file.file_name == name)
}

fn contents(package: &GeneratedDartPackage) -> BTreeMap<&str, &str> {
    package
        .files
        .iter()
        .map(|file| (file.file_name.as_str(), file.contents.as_str()))
        .collect()
}

/// Dart source without its `//` comment lines: prose may say what the package
/// replaces, code may not name it.
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn names_dio(text: &str) -> bool {
    ["Dio", "package:dio", "dio:"]
        .iter()
        .any(|needle| text.contains(needle))
}

#[test]
fn the_default_ref_is_the_release_the_generator_targets() {
    assert_eq!(DEFAULT_FESPALIER_REF, "v0.22.0");
    assert_eq!(
        DartGeneratorConfig::default().fespalier_ref,
        DEFAULT_FESPALIER_REF
    );
}

#[test]
fn no_dio_in_lib_or_the_pubspec_for_any_combination() {
    for fixture in FIXTURES {
        for native_cbor in [false, true] {
            let package = generate(fixture, DartPreset::Fespalier, native_cbor);
            let label = format!("{fixture}/native_cbor={native_cbor}");
            for generated in &package.files {
                if generated.file_name.starts_with("lib/") {
                    assert!(
                        !names_dio(&code_only(&generated.contents)),
                        "{label}: {} names Dio in code",
                        generated.file_name
                    );
                }
            }
            let pubspec = file(&package, "pubspec.yaml");
            assert!(
                !pubspec.to_lowercase().contains("dio"),
                "{label}: the pubspec names dio:\n{pubspec}"
            );
            assert!(
                !has_file(&package, "lib/src/dio_transport.dart"),
                "{label}: dio_transport.dart must not be emitted"
            );
        }
    }
}

#[test]
fn the_pubspec_pins_fespalier_transport_by_git() {
    for fixture in FIXTURES {
        let package = generate(fixture, DartPreset::Fespalier, false);
        let pubspec = file(&package, "pubspec.yaml");
        assert!(
            pubspec.contains(
                "  fespalier_transport:\n    git:\n      url: https://github.com/fespalier/fespalier\n      path: packages/fespalier_transport\n      ref: 'v0.22.0'\n"
            ),
            "{fixture}:\n{pubspec}"
        );
        // Everything else is the riverpod preset's.
        for needle in ["flutter_riverpod:", "riverpod_annotation:", "build_runner:"] {
            assert!(pubspec.contains(needle), "{fixture}: lacks {needle}");
        }
    }
}

#[test]
fn fespalier_ref_overrides_the_pin_and_is_quoted() {
    let package = try_generate("tiny_rest", DartPreset::Fespalier, false, "0123abcd")
        .expect("a commit sha is a valid ref");
    assert!(file(&package, "pubspec.yaml").contains("      ref: '0123abcd'\n"));
    // A quote in the ref cannot break out of the YAML scalar.
    let package = try_generate("tiny_rest", DartPreset::Fespalier, false, "feat/it's")
        .expect("a branch name is a valid ref");
    assert!(file(&package, "pubspec.yaml").contains("      ref: 'feat/it''s'\n"));
    // The ref is the fespalier preset's alone.
    let riverpod = try_generate("tiny_rest", DartPreset::Riverpod, false, "0123abcd")
        .expect("riverpod ignores the ref");
    assert!(!file(&riverpod, "pubspec.yaml").contains("0123abcd"));
}

#[test]
fn a_malformed_fespalier_ref_is_an_error() {
    for bad in ["", "v0.22 .0", "a\nb", "\t"] {
        let error = try_generate("tiny_rest", DartPreset::Fespalier, false, bad)
            .expect_err("a git ref has no whitespace and is not empty");
        assert!(
            matches!(error, DartGeneratorError::InvalidFespalierRef(_)),
            "{bad:?}: {error}"
        );
    }
}

#[test]
fn the_library_exports_the_fespalier_transport_not_dios() {
    for fixture in FIXTURES {
        let package = generate(fixture, DartPreset::Fespalier, false);
        let library = file(&package, &format!("lib/{fixture}_client.dart"));
        assert!(library.contains("export 'src/fespalier_transport.dart';"));
        assert!(!library.contains("dio_transport"));
    }
}

#[test]
fn the_transport_maps_requests_responses_and_failures() {
    let package = generate("tiny_rest", DartPreset::Fespalier, false);
    let transport = file(&package, "lib/src/fespalier_transport.dart");
    for needle in [
        "import 'package:fespalier_transport/fespalier_transport.dart';",
        "class CratestackFespalierTransport implements CratestackHttpTransport {",
        "final HttpTransport transport;",
        "body: request.body,",
        "abortTrigger: TransportZone.merge(",
        "request.abort,",
        "TransportZone.current(),",
        "attributes: request.extra,",
        "on TransportFailure catch (failure)",
        "on TransportStatusError catch (error)",
        "TransportAborted() => CratestackTransportAborted(cause: failure),",
        "TransportUnsent() => CratestackTransportUnsent(cause: failure),",
        "TransportTimedOut() => CratestackTransportTimedOut(cause: failure),",
        "TransportBroken() ||",
        "TransportPortal() ||",
        "TransportUntrusted() => CratestackTransportBroken(cause: failure),",
    ] {
        assert!(transport.contains(needle), "lacks `{needle}`:\n{transport}");
    }
}

#[test]
fn the_providers_default_the_adapter_to_the_http_adapter_over_the_transport() {
    for (fixture, adapter, interface) in [
        (
            "tiny_rest",
            "CratestackHttpAdapter",
            "CratestackClientAdapter",
        ),
        (
            "tiny_rpc",
            "CratestackRpcHttpAdapter",
            "CratestackRpcAdapter",
        ),
    ] {
        let package = generate(fixture, DartPreset::Fespalier, false);
        let client = file(&package, "lib/src/client.dart");
        let prefix = format!("{}Client", to_camel(fixture));
        for needle in [
            format!("final {prefix}TransportProvider = Provider<HttpTransport>((ref) {{"),
            format!(
                "throw UnimplementedError('Override {prefix}TransportProvider with your fespalier_transport chain"
            ),
            format!("final {prefix}BaseUriProvider = Provider<Uri>((ref) {{"),
            format!("throw UnimplementedError('Override {prefix}BaseUriProvider"),
            format!("final {prefix}AdapterProvider = Provider<{interface}>((ref) {{"),
            format!("return {adapter}("),
            format!("CratestackFespalierTransport(ref.watch({prefix}TransportProvider)),"),
            format!("baseUri: ref.watch({prefix}BaseUriProvider),"),
            "import 'package:fespalier_transport/fespalier_transport.dart';".to_owned(),
        ] {
            assert!(
                client.contains(&needle),
                "{fixture}: lacks `{needle}`:\n{client}"
            );
        }
        // The riverpod preset keeps its unimplemented adapter provider.
        let riverpod = generate(fixture, DartPreset::Riverpod, false);
        let riverpod_client = file(&riverpod, "lib/src/client.dart");
        assert!(!riverpod_client.contains("TransportProvider"));
        assert!(riverpod_client.contains(&format!(
            "throw UnimplementedError('Override {prefix}AdapterProvider before reading"
        )));
    }
}

fn to_camel(fixture: &str) -> String {
    let mut parts = fixture.split('_');
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    out
}

/// The fespalier preset is the riverpod preset plus a transport: the same
/// files, with exactly the five that name the transport differing.
#[test]
fn fespalier_differs_from_riverpod_only_where_the_transport_shows() {
    for fixture in FIXTURES {
        let riverpod = generate(fixture, DartPreset::Riverpod, false);
        let fespalier = generate(fixture, DartPreset::Fespalier, false);
        let (riverpod, fespalier) = (contents(&riverpod), contents(&fespalier));

        let mut only_riverpod: Vec<&str> = riverpod
            .keys()
            .copied()
            .filter(|k| !fespalier.contains_key(k))
            .collect();
        let mut only_fespalier: Vec<&str> = fespalier
            .keys()
            .copied()
            .filter(|k| !riverpod.contains_key(k))
            .collect();
        only_riverpod.sort();
        only_fespalier.sort();
        assert_eq!(only_riverpod, ["lib/src/dio_transport.dart"], "{fixture}");
        assert_eq!(
            only_fespalier,
            ["lib/src/fespalier_transport.dart"],
            "{fixture}"
        );

        let library = format!("lib/{fixture}_client.dart");
        let differing: Vec<_> = riverpod
            .iter()
            .filter(|(name, text)| fespalier.get(*name).is_some_and(|other| other != *text))
            .map(|(name, _)| *name)
            .collect();
        let mut expected = vec![
            "README.md",
            library.as_str(),
            "lib/src/client.dart",
            "lib/src/runtime.dart",
            "pubspec.yaml",
        ];
        expected.sort_unstable();
        assert_eq!(differing, expected, "{fixture}");
    }
}

/// The seam, the adapters and the call options are the shared runtime's: the
/// fespalier preset adds a transport to it and changes none of it. Only doc
/// comments in `runtime.dart` differ.
#[test]
fn the_runtime_differs_from_riverpods_only_in_doc_comments() {
    for fixture in FIXTURES {
        let riverpod = generate(fixture, DartPreset::Riverpod, false);
        let fespalier = generate(fixture, DartPreset::Fespalier, false);
        assert_eq!(
            code_only(&strip_doc_comments(file(&riverpod, "lib/src/runtime.dart"))),
            code_only(&strip_doc_comments(file(
                &fespalier,
                "lib/src/runtime.dart"
            ))),
            "{fixture}"
        );
    }
}

fn strip_doc_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("///"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn only_the_fespalier_preset_has_the_transport_file_and_readme_section() {
    for fixture in FIXTURES {
        for preset in [DartPreset::Default, DartPreset::Riverpod] {
            let package = generate(fixture, preset, false);
            assert!(
                !has_file(&package, "lib/src/fespalier_transport.dart"),
                "{preset:?}"
            );
            assert!(
                !file(&package, "README.md").contains("fespalier"),
                "{preset:?}"
            );
        }
        let readme = file(
            &generate(fixture, DartPreset::Fespalier, false),
            "README.md",
        )
        .to_owned();
        for needle in [
            "CratestackFespalierTransport",
            "--preset fespalier --run-build-runner",
            "TransportZone",
            "fespalier packages are git dependencies by design",
            "`--fespalier-ref`",
        ] {
            assert!(
                readme.contains(needle),
                "{fixture}: README lacks `{needle}`"
            );
        }
    }
}

#[test]
fn is_riverpod_layout_covers_riverpod_and_fespalier() {
    assert!(!DartPreset::Default.is_riverpod_layout());
    assert!(DartPreset::Riverpod.is_riverpod_layout());
    assert!(DartPreset::Fespalier.is_riverpod_layout());
}
