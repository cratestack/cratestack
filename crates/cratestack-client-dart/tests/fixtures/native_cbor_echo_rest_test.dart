// cratestack#647 gap closure: this file settles the one thing
// `native_cbor_generator.rs`'s structural (source-text) assertions cannot
// — that a `--native-cbor`-generated REST client's `CratestackCborDioAdapter`
// really encodes a request body and decodes a response body through the
// real, published `cratestack_cbor` package (flutter_rust_bridge-backed on
// this CI runner's Linux x86_64 host), not a plausible-looking template
// edit that never actually round-trips. Mirrors
// `status_override_202_test.dart`'s real `dart:io HttpServer` + real `dio`
// discipline — no fake `CratestackClientAdapter` stand-in.
//
// The server side deliberately still speaks plain `package:cbor` (not
// `cratestack_cbor`): the property under test is specifically whether the
// GENERATED CLIENT's native codec produces/consumes CBOR bytes that are
// correct BY THE INDEPENDENT PURE-DART DECODER'S standard, not whether two
// copies of the same native codec merely agree with themselves.
//
// `just verify-dart` copies this file into a `default`-preset package
// generated from `native_cbor_echo.cstack` with `--native-cbor` and runs it
// with `flutter test`.

import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:native_cbor_echo_rest_verify/native_cbor_echo_rest_verify.dart';

void main() {
  test(
    'generated --native-cbor REST client round-trips a real request and '
    'response through the real cratestack_cbor codec',
    () async {
      Object? capturedRequestBody;
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        final bytes = <int>[];
        await for (final chunk in request) {
          bytes.addAll(chunk);
        }
        // Decoded with the plain pure-Dart `cbor` package, deliberately not
        // `cratestack_cbor` — see the module comment above.
        capturedRequestBody = cratestackNormalizeWire(
          cbor.cbor.decode(Uint8List.fromList(bytes)),
        );

        request.response.statusCode = 200;
        request.response.headers.set('content-type', 'application/cbor');
        request.response.add(
          Uint8List.fromList(
            cbor.cbor.encode({'echo': 'hello from the real native codec'}),
          ),
        );
        await request.response.close();
      });
      addTearDown(server.close);

      final dio = Dio(BaseOptions(baseUrl: 'http://127.0.0.1:${server.port}'));
      final adapter = CratestackCborDioAdapter(dio: dio);
      final client = NativeCborEchoRestVerifyCratestackClient(
        adapter,
        basePath: '',
      );

      final reply = await client.procedures.echo(
        const EchoProcedureArgs(args: EchoArgs(message: 'hello')),
      );

      // Response decode: the server encoded with plain `cbor`, the client
      // decoded with the real `cratestack_cbor` native codec.
      expect(reply.echo, 'hello from the real native codec');

      // Request encode: the client encoded with the real `cratestack_cbor`
      // native codec, the test decoded with plain `cbor` independently.
      // `EchoProcedureArgs.toWire()` wraps the schema's single `args`
      // parameter under that key.
      expect(capturedRequestBody, {'args': {'message': 'hello'}});
    },
  );
}
