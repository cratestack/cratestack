//! cratestack#1204: the generated package has one HTTP seam,
//! `CratestackHttpTransport`, under the adapter interfaces; Dio is one
//! transport among others and is confined to a single file.
//!
//! Structural coverage (source-level). The behavioural proof — wire parity
//! with the retired Dio adapters, typed errors, abort, chunked streaming —
//! runs under `just verify-dart` from `tests/fixtures/wire_parity_{rest,rpc}_test.dart`.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const DIO_TRANSPORT: &str = "lib/src/dio_transport.dart";

fn generate(fixture: &str, preset: DartPreset, native_cbor: bool) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path:?} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: format!("{fixture}_client"),
            preset,
            native_cbor,
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

/// Every default/riverpod x REST/RPC x pure-Dart/native-CBOR combination.
fn every_package() -> Vec<(String, GeneratedDartPackage)> {
    let mut packages = Vec::new();
    for fixture in ["tiny_rest", "tiny_rpc"] {
        for preset in [DartPreset::Default, DartPreset::Riverpod] {
            for native_cbor in [false, true] {
                packages.push((
                    format!("{fixture}/{preset:?}/native_cbor={native_cbor}"),
                    generate(fixture, preset, native_cbor),
                ));
            }
        }
    }
    packages
}

#[test]
fn dio_is_named_only_in_the_transport_file() {
    for (label, package) in every_package() {
        for generated in &package.files {
            let in_lib = generated.file_name.starts_with("lib/");
            if !in_lib || generated.file_name == DIO_TRANSPORT {
                continue;
            }
            // Code, not prose: a `///` example showing how to wrap a Dio is
            // documentation (the riverpod `AdapterProvider` doc must show it).
            let code = generated
                .contents
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for needle in ["Dio", "package:dio", "dio:"] {
                assert!(
                    !code.contains(needle),
                    "{label}: {} names `{needle}` in code — Dio belongs in {DIO_TRANSPORT} only",
                    generated.file_name
                );
            }
        }
        let transport = file(&package, DIO_TRANSPORT);
        assert!(
            transport.contains("import 'package:dio/dio.dart';"),
            "{label}"
        );
        assert!(
            transport.contains("class CratestackDioTransport implements CratestackHttpTransport {"),
            "{label}"
        );
    }
}

#[test]
fn presets_keep_dio_as_their_documented_default_transport() {
    for (label, package) in every_package() {
        let pubspec = file(&package, "pubspec.yaml");
        assert!(pubspec.contains("dio: ^5.8.0+1"), "{label}:\n{pubspec}");
        let library = file(&package, &library_path(&package));
        assert!(
            library.contains("export 'src/dio_transport.dart';"),
            "{label}: the public library must export the Dio transport:\n{library}"
        );
    }
}

fn library_path(package: &GeneratedDartPackage) -> String {
    package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .find(|name| name.starts_with("lib/") && !name[4..].contains('/'))
        .expect("a library entrypoint directly under lib/")
        .to_owned()
}

#[test]
fn every_runtime_declares_the_seam_and_the_sealed_transport_exceptions() {
    for (label, package) in every_package() {
        let runtime = file(&package, "lib/src/runtime.dart");
        for needle in [
            "abstract interface class CratestackHttpTransport {",
            "Future<CratestackHttpResponse> send(CratestackHttpRequest request);",
            "final class CratestackHttpRequest {",
            "final class CratestackHttpResponse {",
            "Future<Uint8List> bytes() async {",
            "sealed class CratestackTransportException implements Exception {",
            "final class CratestackTransportAborted extends CratestackTransportException {",
            "final class CratestackTransportUnsent extends CratestackTransportException {",
            "final class CratestackTransportTimedOut extends CratestackTransportException {",
            "final class CratestackTransportBroken extends CratestackTransportException {",
            "final Future<void>? abort;",
            "enum CratestackWireFormat {",
        ] {
            assert!(
                runtime.contains(needle),
                "{label}: runtime.dart lacks `{needle}`"
            );
        }
        assert!(
            !runtime.contains("DioAdapter"),
            "{label}: a *DioAdapter survived"
        );
    }
}

#[test]
fn rest_runtime_has_the_http_adapter_the_typed_error_and_the_new_call_options() {
    for native_cbor in [false, true] {
        let package = generate("tiny_rest", DartPreset::Default, native_cbor);
        let runtime = file(&package, "lib/src/runtime.dart");
        for needle in [
            "class CratestackHttpAdapter implements CratestackClientAdapter {",
            "class CratestackRestException implements Exception {",
            "String? cratestackCanonicalizeQuery(Map<String, Object?>? queryParameters) {",
            "if (options?.idempotencyKey != null)",
            "'Idempotency-Key': options!.idempotencyKey!,",
            "abort: options?.abort,",
            "if (useRustTransport) cratestackUseRustTransportExtraKey: true,",
        ] {
            assert!(
                runtime.contains(needle),
                "native_cbor={native_cbor}: lacks `{needle}`"
            );
        }
        // Both call-option fields, on the REST options class.
        let options = &runtime[runtime
            .find("class CratestackCallOptions {")
            .expect("REST call options")..];
        assert!(options.contains("final String? idempotencyKey;"));
        assert!(options.contains("final Future<void>? abort;"));
        assert!(!runtime.contains("class CratestackRpcHttpAdapter"));
    }
}

#[test]
fn rpc_runtime_has_the_http_adapter_the_streaming_scanner_and_the_new_call_options() {
    for native_cbor in [false, true] {
        let package = generate("tiny_rpc", DartPreset::Default, native_cbor);
        let runtime = file(&package, "lib/src/runtime.dart");
        for needle in [
            "class CratestackRpcHttpAdapter implements CratestackRpcAdapter {",
            "class CratestackCborSeqScanner implements CborSeqDecoderHandle {",
            "response.body.transform(",
            "CborSeqStreamTransformer(decoder)",
            "abort: options?.abort,",
        ] {
            assert!(
                runtime.contains(needle),
                "native_cbor={native_cbor}: lacks `{needle}`"
            );
        }
        let options = &runtime[runtime
            .find("class CratestackRpcCallOptions {")
            .expect("RPC call options")..];
        assert!(options.contains("final String? idempotencyKey;"));
        assert!(options.contains("final Future<void>? abort;"));
        // `stream()` must not buffer the body any more.
        assert!(
            !runtime.contains("TODO: stream `application/cbor-seq`"),
            "the buffering TODO should be resolved"
        );
        assert!(
            !runtime.contains("class CratestackHttpAdapter implements CratestackClientAdapter")
        );
    }
}

#[test]
fn the_readme_documents_the_seam_and_no_longer_lists_a_missing_typed_error() {
    let rest = generate("tiny_rest", DartPreset::Default, false);
    let readme = file(&rest, "README.md");
    assert!(readme.contains("CratestackHttpAdapter("), "{readme}");
    assert!(readme.contains("CratestackDioTransport(myDio)"), "{readme}");
    assert!(readme.contains("CratestackRestException"), "{readme}");
    assert!(!readme.contains("CratestackDioAdapter"), "{readme}");
    assert!(!readme.contains("CratestackCborDioAdapter"), "{readme}");
    assert!(!readme.contains("typed remote-error"), "{readme}");

    let rpc = generate("tiny_rpc", DartPreset::Default, false);
    let readme = file(&rpc, "README.md");
    assert!(readme.contains("CratestackRpcHttpAdapter("), "{readme}");
    assert!(readme.contains("CratestackRpcException"), "{readme}");
}

#[test]
fn riverpod_adapter_provider_docs_show_the_http_adapter_over_the_dio_transport() {
    let rest = generate("tiny_rest", DartPreset::Riverpod, false);
    let client = file(&rest, "lib/src/client.dart");
    assert!(client.contains("CratestackHttpAdapter(\n///         CratestackDioTransport(dio),"));

    let rpc = generate("tiny_rpc", DartPreset::Riverpod, false);
    let client = file(&rpc, "lib/src/client.dart");
    assert!(client.contains("CratestackRpcHttpAdapter(\n///         CratestackDioTransport(dio),"));
}

/// cratestack#1243: both exceptions type an `AuthProvider` refusal's members,
/// `details` and `WWW-Authenticate` challenges, and `RpcErrorBody` no longer
/// drops the members it does not know. The behavioural proof is
/// `tests/fixtures/auth_refusal_{rest,rpc}_test.dart`, under `just verify-dart`.
#[test]
fn both_exceptions_type_the_refusal_members_and_the_challenges() {
    for (label, package) in every_package() {
        let runtime = file(&package, "lib/src/runtime.dart");
        let common = [
            "Map<String, Object?> _cratestackExtraErrorMembers(Object? body) {",
            "List<String> _cratestackChallenges(Map<String, String> headers) {",
            "this.headers = const <String, String>{},",
            "List<String> get challenges => _cratestackChallenges(headers);",
        ];
        let rest = [
            "String? get code => _envelopeMember('code') as String?;",
            "Map<String, Object?> get extra => _cratestackExtraErrorMembers(body);",
            "        headers: response.headers,\n      );",
        ];
        let rpc = [
            "final Map<String, Object?> extra;",
            "extra: _cratestackExtraErrorMembers(map),",
            "Map<String, Object?> get extra => body.extra;",
            "        headers: response.headers,\n      );",
        ];
        let specific: &[&str] = if label.starts_with("tiny_rest") {
            &rest
        } else {
            &rpc
        };
        for needle in common.iter().chain(specific) {
            assert!(
                runtime.contains(needle),
                "{label}: runtime.dart lacks `{needle}`"
            );
        }
    }
}
