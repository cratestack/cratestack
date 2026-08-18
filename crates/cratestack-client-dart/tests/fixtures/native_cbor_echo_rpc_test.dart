// cratestack#647 gap closure: the RPC transport twin of
// `native_cbor_echo_rest_test.dart` — see that file's module comment for
// the "server speaks plain cbor, client speaks the real cratestack_cbor
// codec" rationale.
//
// This file additionally covers the async exception-decode path
// (`_exceptionFromDio`) that #647's own risk assessment flagged as the part
// "most likely to have missed a call site": a 4xx CBOR error body must be
// awaited and decoded through the real native codec, not just the happy
// path.
//
// `just verify-dart` copies this file into a `default`-preset package
// generated from `native_cbor_echo_rpc.cstack` with `--native-cbor` and
// runs it with `flutter test`.

import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:native_cbor_echo_rpc_verify/native_cbor_echo_rpc_verify.dart';

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
  test(
    'generated --native-cbor RPC client round-trips a real request and '
    'response through the real cratestack_cbor codec',
    () async {
      Object? capturedRequestBody;
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        final bytes = <int>[];
        await for (final chunk in request) {
          bytes.addAll(chunk);
        }
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
      final adapter = CratestackRpcCborDioAdapter(dio: dio);
      final client = NativeCborEchoRpcVerifyCratestackClient(adapter);

      final reply = await client.procedures.echo(
        const EchoProcedureArgs(args: EchoArgs(message: 'hello')),
      );

      expect(reply.echo, 'hello from the real native codec');
      expect(capturedRequestBody, {'args': {'message': 'hello'}});
    },
  );

  test(
    'generated --native-cbor RPC client awaits and decodes a real 4xx CBOR '
    'error body through the real cratestack_cbor codec (the async '
    '_exceptionFromDio rework)',
    () async {
      final server = await _startCborServer(400, {
        'code': 'invalid_argument',
        'message': 'message must not be empty',
      });
      addTearDown(server.close);

      final dio = Dio(BaseOptions(baseUrl: 'http://127.0.0.1:${server.port}'));
      final adapter = CratestackRpcCborDioAdapter(dio: dio);
      final client = NativeCborEchoRpcVerifyCratestackClient(adapter);

      await expectLater(
        client.procedures.echo(
          const EchoProcedureArgs(args: EchoArgs(message: 'hello')),
        ),
        throwsA(
          isA<CratestackRpcException>()
              .having((e) => e.status, 'status', 400)
              .having(
                (e) => e.body.code,
                'body.code',
                'invalid_argument',
              )
              .having(
                (e) => e.body.message,
                'body.message',
                'message must not be empty',
              ),
        ),
      );
    },
  );
}
