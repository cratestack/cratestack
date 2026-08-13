// Real, executable benchmark: cratestack_cbor (flutter_rust_bridge over
// cratestack-codec-cbor, via #[frb(sync)]) vs pure-Dart package:cbor, on
// two realistic payloads. See README.md in this directory for the
// reproduction steps (a small native crate + generated Dart bindings are
// required — neither is committed, matching cratestack#563's "glue is
// generated, not committed" decision) and the numbers this produced.
//
// Run with `dart run bench.dart` (JIT) or compile first for an AOT
// measurement: `dart compile exe bench.dart -o bench_exe && ./bench_exe`.
//
// Set CRATESTACK_CBOR_NATIVE_LIB to the built cdylib path if it isn't at
// the default `../target/release/libcratestack_cbor_bench_native.so`
// relative to this file (see README.md's reproduction steps).

import 'dart:convert';
import 'dart:io';

import 'package:cbor/cbor.dart' as pure_dart_cbor;
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'api.dart';
import 'frb_generated.dart';

/// A single model response: the scalar matrix a generated client actually
/// carries (see ../../tests/cbor_bridge.rs), already JSON-shaped the way
/// `jsonEncode(model.toJson())` would produce it.
Object singleModelPayload() {
  return {
    'id': '11111111-2222-3333-4444-555555555555',
    'title': 'A realistic model payload',
    'body':
        'CBOR encode/decode sits on every request in a generated client, '
        'so this benchmark uses a payload shaped like a typical API '
        'response: a handful of scalar fields plus a small nested list.',
    'pinned': false,
    'completed': true,
    'createdAt': '2026-08-13T00:00:00Z',
    'updatedAt': '2026-08-13T00:00:00Z',
    'amount': '1234.5600',
    'tags': ['cratestack', 'cbor', 'benchmark', 'flutter_rust_bridge'],
    'metadata': {
      'source': 'bench.dart',
      'version': 3,
      'flags': [true, false, true],
    },
  };
}

/// A `find_many`-shaped list page: 50 rows of the same model, plus
/// `PageInfo` — closer to what a real list endpoint returns than a single
/// row is.
Object listPagePayload() {
  return {
    'items': List.generate(
      50,
      (i) => {
        'id': '11111111-2222-3333-4444-55555555555$i',
        'title': 'Item number $i in a realistic list page',
        'body': 'A moderately sized body field representative of a real '
            'API response row, repeated enough to be non-trivial.',
        'pinned': i % 3 == 0,
        'completed': i % 2 == 0,
        'createdAt': '2026-08-13T00:00:00Z',
        'updatedAt': '2026-08-13T00:00:00Z',
        'amount': '${1000 + i}.5600',
        'tags': ['cratestack', 'cbor', 'row-$i'],
      },
    ),
    'pageInfo': {'hasNextPage': true, 'hasPreviousPage': false},
  };
}

void runScenario(String label, Object payload, int warmup, int iterations) {
  final jsonText = jsonEncode(payload);

  for (var i = 0; i < warmup; i++) {
    final encoded = pure_dart_cbor.cbor.encode(pure_dart_cbor.CborValue(payload));
    pure_dart_cbor.cbor.decode(encoded);
  }
  final pureDartEncoded =
      pure_dart_cbor.cbor.encode(pure_dart_cbor.CborValue(payload));
  final pureDartSw = Stopwatch()..start();
  for (var i = 0; i < iterations; i++) {
    final encoded = pure_dart_cbor.cbor.encode(pure_dart_cbor.CborValue(payload));
    pure_dart_cbor.cbor.decode(encoded);
  }
  pureDartSw.stop();

  for (var i = 0; i < warmup; i++) {
    final encoded = cborEncodeJson(json: jsonText);
    cborDecodeJson(bytes: encoded);
  }
  final bridgeSw = Stopwatch()..start();
  for (var i = 0; i < iterations; i++) {
    final encoded = cborEncodeJson(json: jsonText);
    cborDecodeJson(bytes: encoded);
  }
  bridgeSw.stop();

  final pureDartTotalMs = pureDartSw.elapsedMicroseconds / 1000;
  final bridgeTotalMs = bridgeSw.elapsedMicroseconds / 1000;

  print('-- $label --');
  print('payload: ${utf8.encode(jsonText).length} bytes (JSON), '
      '${pureDartEncoded.length} bytes (CBOR)');
  print('iterations: $iterations (encode+decode per iteration)');
  print('pure-Dart package:cbor : ${pureDartTotalMs.toStringAsFixed(1)} ms '
      'total, ${(pureDartSw.elapsedMicroseconds / iterations).toStringAsFixed(2)} us/iter');
  print('cratestack_cbor (frb)  : ${bridgeTotalMs.toStringAsFixed(1)} ms '
      'total, ${(bridgeSw.elapsedMicroseconds / iterations).toStringAsFixed(2)} us/iter');
  print('speedup: ${(pureDartTotalMs / bridgeTotalMs).toStringAsFixed(2)}x');
  print('');
}

Future<void> main() async {
  final libPath = Platform.environment['CRATESTACK_CBOR_NATIVE_LIB'] ??
      '../target/release/libcratestack_cbor_bench_native.so';
  await RustLib.init(externalLibrary: ExternalLibrary.open(libPath));

  runScenario('single model, 11 scalar fields', singleModelPayload(), 500, 50000);
  runScenario('list page, 50 rows', listPagePayload(), 200, 10000);
}
