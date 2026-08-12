// cratestack#407 (AC5): the Dart half of the generated-client verification.
//
// The Rust client was already proven end-to-end
// (`crates/cratestack-client/tests/status_attribute_client_round_trip.rs`):
// a mock server answering with a bare `202` (never a `200` anywhere in the
// exchange) round-trips through the generated call site as `Ok(..)`, and a
// `500` still surfaces as an error. The Dart client was inspected — no
// explicit `validateStatus` override was found in the REST-runtime
// templates, from which it was *inferred* (never run) that Dio's own
// default `validateStatus` (`200 <= status < 300`) applies unmodified.
// This file settles that with a real generated client hitting a real
// `dart:io HttpServer`, not inference.
//
// `just verify-dart` copies this file into BOTH a `default`-preset and a
// `riverpod`-preset package generated from
// `status_override.cstack` (same fixture, same `--library-name
// dart_status_verify` for both, so the generated class names/imports are
// identical and this one file works unmodified against either package) and
// runs it with `flutter test`. Both presets talk HTTP directly via `dio` —
// neither goes through a Rust FFI bridge — so both are exercised
// independently; RPC transport is out of scope (`@status` is rejected at
// schema-compile time under `transport rpc`, so there is no RPC path to
// check here).

import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:dart_status_verify/dart_status_verify.dart';

/// A real HTTP/1.1 server (`dart:io`, not a fake `CratestackClientAdapter`)
/// so this test exercises Dio's actual `validateStatus` behavior — the
/// exact thing the prior source-reading inference was never checked
/// against.
Future<HttpServer> _startCborServer(
  int status,
  Map<String, Object?> body,
) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((request) async {
    request.response.statusCode = status;
    request.response.headers.set('content-type', 'application/cbor');
    request.response.add(Uint8List.fromList(cbor.cbor.encode(body)));
    await request.response.close();
  });
  return server;
}

void main() {
  test('generated Dart client treats a declared @status(202) response as '
      'success, not an error (no 200 anywhere in this exchange)', () async {
    final server = await _startCborServer(202, {'echo': 'hello'});
    addTearDown(server.close);

    final dio = Dio(BaseOptions(baseUrl: 'http://127.0.0.1:${server.port}'));
    final adapter = CratestackCborDioAdapter(dio: dio);
    final client = DartStatusVerifyCratestackClient(adapter, basePath: '');

    final reply = await client.procedures.submit(
      const SubmitProcedureArgs(args: SubmitArgs(message: 'hello')),
    );

    expect(reply.echo, 'hello');
  });

  test(
    'generated Dart client still treats a 5xx as an error (negative '
    'control — proves the assertion above is exercising real status-code '
    'gating, not a client that accepts every response unconditionally)',
    () async {
      final server = await _startCborServer(500, {'echo': 'hello'});
      addTearDown(server.close);

      final dio = Dio(BaseOptions(baseUrl: 'http://127.0.0.1:${server.port}'));
      final adapter = CratestackCborDioAdapter(dio: dio);
      final client = DartStatusVerifyCratestackClient(adapter, basePath: '');

      await expectLater(
        client.procedures.submit(
          const SubmitProcedureArgs(args: SubmitArgs(message: 'hello')),
        ),
        throwsA(isA<DioException>()),
      );
    },
  );
}
