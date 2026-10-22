// RPC twin of `wire_parity_rest_test.dart` (cratestack#1204): the RPC adapter
// written over the HTTP seam sends exactly what the retired
// `CratestackRpcDioAdapter` / `CratestackRpcCborDioAdapter` sent, maps errors
// as before, and streams a cbor-seq answer frame by frame.
//
// `wire-golden/rpc-{cbor,json}.json` were captured from the pre-seam Dio
// adapters. `just verify-dart` copies this file, `wire_parity_rpc_ops.dart`
// and `wire-golden/` into the default-preset package generated from
// `ci_rpc.cstack` (library name `dart_verify_ci_rpc`).

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'wire_parity_rpc_ops.dart';

class RecordingTransport implements CratestackHttpTransport {
  RecordingTransport([this.respond]);

  final CratestackHttpResponse Function(CratestackHttpRequest)? respond;
  final List<CratestackHttpRequest> requests = [];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return respond?.call(request) ?? CratestackHttpResponse.buffered(status: 204);
  }
}

String _hex(List<int>? bytes) => (bytes ?? const <int>[])
    .map((byte) => byte.toRadixString(16).padLeft(2, '0'))
    .join();

Map<String, Object?> _normalize(CratestackHttpRequest request) {
  final headers = <String, String>{
    for (final entry in request.headers.entries)
      entry.key.toLowerCase(): entry.value == cratestackSchemaSha256
          ? '<schema-sha>'
          : entry.value,
  };
  return {
    'method': request.method,
    'uri': request.uri.toString(),
    'headers': Map.fromEntries(
      headers.entries.toList()..sort((a, b) => a.key.compareTo(b.key)),
    ),
    'bodyHex': _hex(request.body),
  };
}

CratestackHttpResponse _answer(
  int status,
  String contentType,
  List<int> body,
) => CratestackHttpResponse.buffered(
  status: status,
  headers: {'Content-Type': contentType},
  body: body,
);

CratestackRpcHttpAdapter _adapter(
  CratestackHttpTransport transport, {
  CratestackWireFormat format = CratestackWireFormat.cbor,
}) => CratestackRpcHttpAdapter(
  transport,
  baseUri: Uri.parse('http://127.0.0.1:9'),
  format: format,
);

Future<HttpServer> _serve(
  Future<void> Function(HttpRequest request) handle,
) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen(handle);
  addTearDown(() => server.close(force: true));
  return server;
}

/// One `application/cbor-seq` item per value.
Uint8List _seq(Iterable<Object?> values) {
  final out = BytesBuilder();
  for (final value in values) {
    out.add(cbor.cbor.encode(value));
  }
  return out.takeBytes();
}

void main() {
  group('wire parity with the retired Dio adapters', () {
    for (final format in CratestackWireFormat.values) {
      test('${format.name}: every RPC operation sends the golden request', () async {
        final golden = jsonDecode(
          File('test/wire-golden/rpc-${format.name}.json').readAsStringSync(),
        );
        final transport = RecordingTransport();
        await runRpcOps(
          DartVerifyCiRpcCratestackClient(_adapter(transport, format: format)),
        );
        expect(
          transport.requests.map(_normalize).toList(),
          equals(golden),
        );
      });
    }
  });

  group('responses', () {
    test('call decodes a CBOR answer and a JSON answer', () async {
      expect(
        await _adapter(
          RecordingTransport(
            (_) => _answer(200, 'application/cbor', cbor.cbor.encode({'id': 1})),
          ),
        ).call('model.Author.get', {'id': 1}),
        {'id': 1},
      );
      expect(
        await _adapter(
          RecordingTransport(
            (_) => _answer(200, 'application/json', utf8.encode('{"id":2}')),
          ),
          format: CratestackWireFormat.json,
        ).call('model.Author.get', {'id': 2}),
        {'id': 2},
      );
    });

    test('batch decodes frames, including per-frame errors', () async {
      final frames = await _adapter(
        RecordingTransport(
          (_) => _answer(
            200,
            'application/cbor',
            cbor.cbor.encode([
              {'id': 1, 'output': {'id': 1}},
              {
                'id': 2,
                'error': {'code': 'not_found', 'message': 'no such row'},
              },
            ]),
          ),
        ),
      ).batch(const [
        RpcRequest(id: 1, op: 'model.Author.get', input: {'id': 1}),
        RpcRequest(id: 2, op: 'model.Author.get', input: {'id': 9}),
      ]);
      expect(frames, hasLength(2));
      expect(frames[0].output, {'id': 1});
      expect(frames[1].error?.code, 'not_found');
    });
  });

  group('errors', () {
    Future<Object> failure(CratestackHttpResponse response) async {
      try {
        await _adapter(RecordingTransport((_) => response)).call('x', {});
      } catch (error) {
        return error;
      }
      fail('expected the call to throw');
    }

    test('a CBOR error body becomes a CratestackRpcException, as before', () async {
      final error = await failure(
        _answer(
          400,
          'application/cbor',
          cbor.cbor.encode({
            'code': 'invalid_argument',
            'message': 'message must not be empty',
            'details': {'field': 'message'},
          }),
        ),
      );
      expect(
        error,
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 400)
            .having((e) => e.code, 'code', 'invalid_argument')
            .having((e) => e.message, 'message', 'message must not be empty')
            .having((e) => e.details, 'details', {'field': 'message'}),
      );
    });

    test('a JSON error body becomes a CratestackRpcException, as before', () async {
      final error = await failure(
        _answer(
          404,
          'application/json',
          utf8.encode('{"code":"not_found","message":"gone"}'),
        ),
      );
      expect(
        error,
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 404)
            .having((e) => e.code, 'code', 'not_found'),
      );
    });

    test('an error with no usable body is code internal and keeps the status', () async {
      final empty = await failure(CratestackHttpResponse.buffered(status: 500));
      expect(
        empty,
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 500)
            .having((e) => e.code, 'code', CratestackRpcErrorCodes.internal)
            .having((e) => e.message, 'message', 'HTTP 500'),
      );
      final text = await failure(_answer(502, 'text/plain', utf8.encode('Bad Gateway')));
      expect(
        text,
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 502)
            .having((e) => e.message, 'message', 'Bad Gateway'),
      );
    });

    test('a transport failure passes through untouched', () async {
      const timedOut = CratestackTransportTimedOut(message: 'slow');
      await expectLater(
        _adapter(_ThrowingTransport(timedOut)).call('x', {}),
        throwsA(same(timedOut)),
      );
    });
  });

  group('stream', () {
    CratestackHttpResponse seqResponse(Stream<Uint8List> body, {int status = 200}) =>
        CratestackHttpResponse(
          status: status,
          headers: {'Content-Type': 'application/cbor-seq'},
          body: body,
        );

    test('cbor-seq items arrive before the body ends', () async {
      final body = StreamController<Uint8List>();
      final adapter = _adapter(RecordingTransport((_) => seqResponse(body.stream)));
      final items = StreamIterator(adapter.stream('model.Post.list', {}));

      final wire = _seq([
        {'id': 1},
        {'id': 2, 'title': 'a longer second item'},
      ]);
      // First chunk: item 1 whole and the first bytes of item 2.
      final split = _seq([
        {'id': 1},
      ]).length + 3;
      body.add(Uint8List.sublistView(wire, 0, split));

      expect(
        await items.moveNext().timeout(const Duration(seconds: 5)),
        isTrue,
        reason: 'the first frame must not wait for the end of the body',
      );
      expect(items.current, {'id': 1});

      body.add(Uint8List.sublistView(wire, split));
      // Not awaited: `close()` completes only once the (paused) iterator has
      // consumed the done event.
      unawaited(body.close());
      expect(await items.moveNext(), isTrue);
      expect(items.current, {'id': 2, 'title': 'a longer second item'});
      expect(await items.moveNext(), isFalse);
    });

    test('items split across tiny chunks still decode in order', () async {
      final wire = _seq([
        {'a': 1},
        'text',
        [1, 2, 3],
      ]);
      final adapter = _adapter(
        RecordingTransport(
          (_) => seqResponse(
            Stream.fromIterable([
              for (var i = 0; i < wire.length; i += 2)
                Uint8List.sublistView(wire, i, i + 2 > wire.length ? wire.length : i + 2),
            ]),
          ),
        ),
      );
      expect(await adapter.stream('x', {}).toList(), [
        {'a': 1},
        'text',
        [1, 2, 3],
      ]);
    });

    test('the stream-error sentinel surfaces as a CratestackRpcException', () async {
      final wire = BytesBuilder()
        ..add(_seq([
          {'id': 1},
        ]))
        // Tag(48900, RpcErrorBody)
        ..add([0xd9, 0xbf, 0x04])
        ..add(cbor.cbor.encode({'code': 'unavailable', 'message': 'shard went away'}));
      final adapter = _adapter(
        RecordingTransport((_) => seqResponse(Stream.value(wire.takeBytes()))),
      );
      final received = <Object?>[];
      await expectLater(
        adapter.stream('x', {}).forEach(received.add),
        throwsA(
          isA<CratestackRpcException>()
              .having((e) => e.code, 'code', 'unavailable')
              .having((e) => e.message, 'message', 'shard went away'),
        ),
      );
      expect(received, [
        {'id': 1},
      ]);
    });

    test('a body that ends mid-item is a FormatException', () async {
      final wire = _seq([
        {'id': 1, 'title': 'truncated'},
      ]);
      final adapter = _adapter(
        RecordingTransport(
          (_) => seqResponse(Stream.value(Uint8List.sublistView(wire, 0, wire.length - 2))),
        ),
      );
      await expectLater(adapter.stream('x', {}).toList(), throwsFormatException);
    });

    test('a single-array answer still streams its items', () async {
      expect(
        await _adapter(
          RecordingTransport(
            (_) => _answer(200, 'application/cbor', cbor.cbor.encode([{'id': 1}, {'id': 2}])),
          ),
        ).stream('x', {}).toList(),
        [
          {'id': 1},
          {'id': 2},
        ],
      );
      expect(
        await _adapter(
          RecordingTransport(
            (_) => _answer(200, 'application/json', utf8.encode('[1,2]')),
          ),
          format: CratestackWireFormat.json,
        ).stream('x', {}).toList(),
        [1, 2],
      );
    });

    test('an HTTP error before the first frame is a CratestackRpcException', () async {
      final adapter = _adapter(
        RecordingTransport(
          (_) => _answer(401, 'application/json', utf8.encode('{"code":"unauthenticated","message":"no"}')),
        ),
      );
      await expectLater(
        adapter.stream('x', {}).toList(),
        throwsA(isA<CratestackRpcException>().having((e) => e.status, 'status', 401)),
      );
    });

    test('cancelling the subscription cancels the body', () async {
      var cancelled = false;
      final body = StreamController<Uint8List>(onCancel: () => cancelled = true);
      final adapter = _adapter(RecordingTransport((_) => seqResponse(body.stream)));
      final items = StreamIterator(adapter.stream('x', {}));
      body.add(_seq([1]));
      expect(await items.moveNext(), isTrue);
      await items.cancel();
      expect(cancelled, isTrue);
    });
  });

  group('through CratestackDioTransport', () {
    CratestackRpcHttpAdapter dioAdapter(HttpServer server, {Dio? dio}) =>
        CratestackRpcHttpAdapter(
          CratestackDioTransport(dio ?? Dio()),
          baseUri: Uri.parse('http://127.0.0.1:${server.port}'),
        );

    test('a refused connection is CratestackTransportUnsent, never a DioException', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      final adapter = dioAdapter(server);
      await server.close(force: true);
      await expectLater(
        adapter.call('x', {}),
        throwsA(isA<CratestackTransportUnsent>()),
      );
    });

    test('a receive timeout is CratestackTransportTimedOut', () async {
      final server = await _serve((request) async {});
      final dio = Dio(BaseOptions(receiveTimeout: const Duration(milliseconds: 200)));
      await expectLater(
        dioAdapter(server, dio: dio).call('x', {}),
        throwsA(isA<CratestackTransportTimedOut>()),
      );
    });

    test('abort cancels the send', () async {
      final requestSeen = Completer<void>();
      final server = await _serve((request) async {
        requestSeen.complete();
      });
      final abort = Completer<void>();
      final call = dioAdapter(server).call(
        'x',
        {},
        options: CratestackRpcCallOptions(abort: abort.future),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await requestSeen.future;
      abort.complete();
      await outcome;
    });

    test('abort mid-stream ends the stream with CratestackTransportAborted', () async {
      final server = await _serve((request) async {
        request.response.headers.set('content-type', 'application/cbor-seq');
        request.response.bufferOutput = false;
        request.response.add(_seq([1]));
        await request.response.flush();
        // Keep the body open.
      });
      final abort = Completer<void>();
      final items = StreamIterator(
        dioAdapter(server).stream(
          'x',
          {},
          options: CratestackRpcCallOptions(abort: abort.future),
        ),
      );
      expect(await items.moveNext().timeout(const Duration(seconds: 5)), isTrue);
      expect(items.current, 1);
      abort.complete();
      await expectLater(
        items.moveNext().timeout(const Duration(seconds: 5)),
        throwsA(isA<CratestackTransportAborted>()),
      );
    });

    test('a chunked cbor-seq answer streams over a real socket', () async {
      final server = await _serve((request) async {
        request.response.headers.set('content-type', 'application/cbor-seq');
        request.response.bufferOutput = false;
        for (final id in [1, 2, 3]) {
          request.response.add(_seq([
            {'id': id},
          ]));
          await request.response.flush();
        }
        await request.response.close();
      });
      expect(await dioAdapter(server).stream('x', {}).toList(), [
        {'id': 1},
        {'id': 2},
        {'id': 3},
      ]);
    });

    test('the call options reach the server as headers', () async {
      String? idempotency;
      String? schemaSha;
      final server = await _serve((request) async {
        idempotency = request.headers.value('idempotency-key');
        schemaSha = request.headers.value('x-cratestack-schema-sha');
        request.response.statusCode = 204;
        await request.response.close();
      });
      await dioAdapter(server).call(
        'x',
        {},
        options: const CratestackRpcCallOptions(idempotencyKey: 'abc'),
      );
      expect(idempotency, 'abc');
      expect(schemaSha, cratestackSchemaSha256);
    });
  });
}

class _ThrowingTransport implements CratestackHttpTransport {
  const _ThrowingTransport(this.error);

  final Object error;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) =>
      Future<CratestackHttpResponse>.error(error);
}
