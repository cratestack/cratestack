//! Real `flutter pub get` + `flutter test` proof that the generated Dart
//! client's `BigInt` support (ADR 0019, PR B9) behaves as documented, on the
//! runtimes where the difference between a right and a wrong decode is
//! observable. `tests/bigint_generator.rs` proves the generated *text*; this
//! proves what the text does. Modelled on `tests/decimal_round_trip.rs`.
//!
//! The values are the ones ADR 0019 names: `i64::MAX`, `i64::MIN` and
//! `2^53 + 1`, the smallest positive integer a double cannot hold. A `BigInt`
//! decoded through `int` (`int.parse`, `jsonDecode`, `(x as num).toInt()`) is
//! exact on the Dart VM and on dart2wasm and rounds on dart2js, which is
//! Flutter web's JavaScript build. So the same Dart suites run three times:
//!
//! | runtime   | command                                    |
//! |-----------|--------------------------------------------|
//! | Dart VM   | `flutter test`                             |
//! | dart2js   | `flutter test --platform chrome`           |
//! | dart2wasm | `flutter test --platform chrome --wasm`    |
//!
//! The two web legs need a Chrome (`CHROME_EXECUTABLE`, or the usual install
//! paths) and, for wasm, a Flutter whose `test` knows `--wasm`; without them
//! they print a `SKIPPED` line, and `CRATESTACK_REQUIRE_DART_RUNTIMES=1` turns
//! that into a failure. The whole file skips, loudly, when `flutter` is not on
//! `PATH`. No Rust CI job in this repo provisions Flutter, so neither of these
//! runs there; `just verify-dart` is the Flutter-provisioned job.
//!
//! Four packages are generated, because the transport and the preset each have
//! their own generated code:
//!
//! | test                      | transport | preset   | codec            |
//! |---------------------------|-----------|----------|------------------|
//! | `..._rest_client`         | REST      | default  | pure `package:cbor` |
//! | `..._rpc_client`          | RPC       | default  | pure `package:cbor` |
//! | `..._riverpod_*`          | REST, RPC | riverpod | pure `package:cbor` |
//! | `..._native_..._codec`    | REST      | default  | `cratestack_cbor` native |
//!
//! Beyond the runtime matrix there are two codec legs, both on the VM because
//! they need `dart:io`'s `HttpServer`: pure-Dart `package:cbor` and the
//! `cratestack_cbor` native codec (flutter_rust_bridge over a JSON-text
//! boundary), each driving the generated client against pinned CBOR bytes.
//! The native leg needs the vendored library, which the published package
//! ships for dev-mode `flutter test` on linux-x64 only; elsewhere point
//! `CRATESTACK_CBOR_NATIVE_LIB` at a built `libcratestack_client_flutter`, or
//! it prints `SKIPPED`. The web codec of `cratestack_cbor` (wasm-bindgen over
//! JS `JSON.parse`) is covered in the `cratestack_cbor` package's own
//! `dart test -p chrome` suite, `just cbor-verify-package`.
//!
//! The riverpod legs generate `@riverpod` providers, so they run
//! `build_runner` with the preset's pinned `riverpod_generator`; see
//! `bigint_harness` for the one Flutter-channel conflict that makes them skip.

mod bigint_harness;

use bigint_harness::{Package, Runtime, flutter_available};
use cratestack_client_dart::DartPreset;

const REST_TEST: &str = "bigint_round_trip_test.dart";
const RPC_TEST: &str = "bigint_rpc_round_trip_test.dart";
const ECHO_TEST: &str = "bigint_native_cbor_echo_rest_test.dart";
const RIVERPOD_TEST: &str = "bigint_riverpod_test.dart";

fn skip_without_flutter(test: &str) -> bool {
    if flutter_available() {
        return false;
    }
    eprintln!(
        "skipping {test}: `flutter` not on PATH (expected in this repo's Rust-only CI jobs; \
         see tests/bigint_round_trip.rs's module doc)"
    );
    true
}

/// Resolves and builds `package`, or prints why it could not and returns
/// `false` (a failure instead when `CRATESTACK_REQUIRE_DART_RUNTIMES` is set).
fn resolve_or_skip(package: &Package, test: &str) -> bool {
    match package.resolve_and_build() {
        Ok(()) => true,
        Err(reason) => {
            assert!(
                !bigint_harness::require_runtimes(),
                "CRATESTACK_REQUIRE_DART_RUNTIMES is set but {test} cannot run here: {reason}"
            );
            eprintln!("SKIPPED {test}: {reason}");
            false
        }
    }
}

/// The REST client, default preset, pure-Dart `package:cbor`. One generated
/// package serves the runtime matrix and the pure-cbor wire leg, because the
/// work that dominates is `pub get` and `build_runner`, not the tests.
#[test]
fn bigint_round_trips_through_the_generated_rest_client() {
    if skip_without_flutter("bigint_round_trips_through_the_generated_rest_client") {
        return;
    }
    let package = bigint_harness::generate(
        "bigint-rest",
        "bigint_scalar",
        "bigint_round_trip_check",
        DartPreset::Default,
        false,
    );
    package.add_test(REST_TEST, REST_TEST);
    // The echo test imports the package under its own name; here that is the
    // one this package already has.
    package.add_test_with_library(
        ECHO_TEST,
        ECHO_TEST,
        "bigint_native_cbor_echo_rest_verify",
        "bigint_round_trip_check",
    );
    assert!(resolve_or_skip(&package, "the default-preset REST client"));
    package.analyze();

    for runtime in Runtime::ALL {
        package.run_on(REST_TEST, runtime);
    }
    // Pure `package:cbor`, VM only (`dart:io`).
    package.run_on(ECHO_TEST, Runtime::Vm);
    package.cleanup();
}

/// The RPC client: a `BigInt` primary key is a frame field, not a path
/// segment, so it needs its own conversion and its own proof.
#[test]
fn bigint_round_trips_through_the_generated_rpc_client() {
    if skip_without_flutter("bigint_round_trips_through_the_generated_rpc_client") {
        return;
    }
    let package = bigint_harness::generate(
        "bigint-rpc",
        "bigint_scalar_rpc",
        "bigint_rpc_round_trip_check",
        DartPreset::Default,
        false,
    );
    package.add_test(RPC_TEST, RPC_TEST);
    assert!(resolve_or_skip(&package, "the default-preset RPC client"));
    package.analyze();

    for runtime in Runtime::ALL {
        package.run_on(RPC_TEST, runtime);
    }
    package.cleanup();
}

/// The riverpod preset, REST: the same behaviour suite, plus what only this
/// preset adds: dart_mappable `==`/`hashCode` over `BigInt` fields, and a
/// `BigInt` key as a `@riverpod` family argument.
#[test]
fn bigint_round_trips_through_the_riverpod_preset_rest_client() {
    if skip_without_flutter("bigint_round_trips_through_the_riverpod_preset_rest_client") {
        return;
    }
    let package = bigint_harness::generate(
        "bigint-riverpod-rest",
        "bigint_scalar",
        "bigint_round_trip_check",
        DartPreset::Riverpod,
        false,
    );
    package.add_test(REST_TEST, REST_TEST);
    package.add_test(RIVERPOD_TEST, RIVERPOD_TEST);
    if !resolve_or_skip(&package, "the riverpod-preset REST client") {
        package.cleanup();
        return;
    }

    package.analyze();
    for runtime in Runtime::ALL {
        package.run_on(REST_TEST, runtime);
        package.run_on(RIVERPOD_TEST, runtime);
    }
    package.cleanup();
}

/// The riverpod preset, RPC: its own `rpc_model.dart.j2` writes the `{'id': ..}`
/// frames, so it needs the string conversion as well.
#[test]
fn bigint_round_trips_through_the_riverpod_preset_rpc_client() {
    if skip_without_flutter("bigint_round_trips_through_the_riverpod_preset_rpc_client") {
        return;
    }
    let package = bigint_harness::generate(
        "bigint-riverpod-rpc",
        "bigint_scalar_rpc",
        "bigint_rpc_round_trip_check",
        DartPreset::Riverpod,
        false,
    );
    package.add_test(RPC_TEST, RPC_TEST);
    if !resolve_or_skip(&package, "the riverpod-preset RPC client") {
        package.cleanup();
        return;
    }
    package.analyze();

    for runtime in Runtime::ALL {
        package.run_on(RPC_TEST, runtime);
    }
    package.cleanup();
}

/// The same pinned bytes through the `cratestack_cbor` native codec, resolved
/// from pub.dev as a real consumer would. VM only: the native backend is a
/// vendored dynamic library.
#[test]
fn bigint_bytes_round_trip_through_the_native_cratestack_cbor_codec() {
    if skip_without_flutter("bigint_bytes_round_trip_through_the_native_cratestack_cbor_codec") {
        return;
    }
    if let Err(reason) = bigint_harness::native_cbor_available() {
        assert!(
            !bigint_harness::require_runtimes(),
            "CRATESTACK_REQUIRE_DART_RUNTIMES is set but the native codec cannot run here: {reason}"
        );
        eprintln!("SKIPPED {ECHO_TEST} on the cratestack_cbor native codec: {reason}");
        return;
    }
    let package = bigint_harness::generate(
        "bigint-native-cbor",
        "bigint_scalar",
        "bigint_native_cbor_echo_rest_verify",
        DartPreset::Default,
        true,
    );
    package.add_cbor_dev_dependency();
    package.allow_any_cratestack_cbor();
    package.add_test(ECHO_TEST, ECHO_TEST);
    assert!(resolve_or_skip(&package, "the native-cbor REST client"));
    package.analyze();

    package.run_on(ECHO_TEST, Runtime::Vm);
    package.cleanup();
}
