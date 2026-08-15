// Real, executed proof (cratestack#563) that the flutter_rust_bridge glue
// generated from crates/cratestack-client-flutter actually works end to
// end — not just that `flutter_rust_bridge_codegen generate` exits 0.
// Loads the compiled cdylib, calls the generated `encodeJson`/`decodeJson`
// synchronously (both are `#[frb(sync)]` — see `../src/cbor/mod.rs`), and
// asserts a round trip plus byte-identical output against the same hex
// fixtures `../src/cbor/mod.rs`'s and `cratestack-cbor-napi`'s own test
// suites assert, so this is checked against the same known-good bytes as
// the other two platform bindings, not just self-consistency.
//
// Usage: `dart run verify_round_trip.dart` from this directory, after
// `dart pub get` and building the cdylib with the `frb-glue` feature
// (`just frb-generate crates/cratestack-client-flutter` then `cargo build
// -p cratestack-client-flutter --features frb-glue --release`).
//
// Set CRATESTACK_CLIENT_FLUTTER_NATIVE_LIB to override the default
// `../../../target/release/libcratestack_client_flutter.so` path (Linux
// naming; adjust for other platforms — this script is a CI/local
// verification harness, not a cross-platform loader, matching the
// benches/cbor_bridge/bench.dart precedent).

import 'dart:convert';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'package:cratestack_cbor_frb_verification/src/rust/frb_generated.dart';
import 'package:cratestack_cbor_frb_verification/src/rust/cbor.dart' as cbor;
import 'package:cratestack_cbor_frb_verification/src/rust/types.dart';

void expect(bool condition, String message) {
  if (!condition) {
    stderr.writeln('FAIL: $message');
    exit(1);
  }
  print('ok: $message');
}

String hex(List<int> bytes) =>
    bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

Future<void> main() async {
  final libPath = Platform.environment['CRATESTACK_CLIENT_FLUTTER_NATIVE_LIB'] ??
      '../../../target/release/libcratestack_client_flutter.so';
  if (!File(libPath).existsSync()) {
    stderr.writeln(
      'Native library not found at $libPath. Build it first: '
      'cargo build -p cratestack-client-flutter --features frb-glue --release',
    );
    exit(1);
  }
  await RustLib.init(externalLibrary: ExternalLibrary.open(libPath));

  // 1. Basic round trip: encode -> decode returns the same JSON value.
  final roundTripInput = '{"cratestack":["cool","stack"],"n":42}';
  final encoded = cbor.encodeJson(json: roundTripInput);
  final decoded = cbor.decodeJson(bytes: encoded);
  expect(
    jsonDecode(decoded).toString() == jsonDecode(roundTripInput).toString(),
    'encodeJson -> decodeJson round-trips the JSON value',
  );

  // 2. Byte-identical to the fixtures `src/cbor/mod.rs`'s
  // `fixture_bytes_shared_with_the_napi_and_wasm_cross_language_tests_stay_correct`
  // and `cratestack-cbor-napi`'s equivalent test assert directly against
  // `CborCodec` — proving the Dart-facing bridge produces the exact same
  // wire bytes as the Rust codec and the other two platform bindings, not
  // just something self-consistent.
  expect(
    hex(cbor.encodeJson(json: jsonEncode(['cool', 'stack']))) ==
        '8264636f6f6c65737461636b',
    'encodeJson(["cool","stack"]) matches the shared cross-binding fixture',
  );
  expect(
    hex(cbor.encodeJson(
          json: jsonEncode({'cratestack': ['cool', 'stack'], 'n': 42, 'ok': true}),
        )) ==
        'a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5',
    'encodeJson(object) matches the shared cross-binding fixture',
  );

  // 3. Errors surface as catchable Dart exceptions, not crashes/panics —
  // `FlutterRuntimeError implements FrbException` (generated
  // lib/src/rust/types.dart), so a malformed decode throws rather than
  // aborting the process.
  var threw = false;
  try {
    cbor.decodeJson(bytes: [0x1b]);
  } on FlutterRuntimeError catch (_) {
    threw = true;
  }
  expect(threw, 'malformed CBOR bytes throw a catchable FlutterRuntimeError');

  print('\nAll flutter_rust_bridge round-trip checks passed.');
}
