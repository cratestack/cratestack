// cratestack#1204: proof that the REST adapter written over the HTTP seam
// sends exactly what the retired `CratestackDioAdapter` /
// `CratestackCborDioAdapter` sent, and the behaviours the seam adds on top.
//
// `wire-golden/rest-{cbor,json}.json` were captured from the pre-seam Dio
// adapters (git history: the commit that introduced the seam) by driving
// `wire_parity_rest_ops.dart` through a recording Dio. This test replays the
// same operations through a recording `CratestackHttpTransport` and compares
// method, URI (path + canonical query), headers and body bytes, request by
// request.
//
// `just verify-dart` copies this file, `wire_parity_rest_ops.dart` and
// `wire-golden/` into the default-preset package generated from
// `ci_rest.cstack` (library name `dart_verify_ci_rest`).

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'wire_parity_rest_ops.dart';

/// Records every request and answers with [respond] (a bare `204` by default).
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

CratestackHttpAdapter _adapter(
  CratestackHttpTransport transport, {
  CratestackWireFormat format = CratestackWireFormat.cbor,
}) => CratestackHttpAdapter(
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

void main() {
  group('wire parity with the retired Dio adapters', () {
    for (final format in CratestackWireFormat.values) {
      test('${format.name}: every REST operation sends the golden request', () async {
        final golden = jsonDecode(
          File('test/wire-golden/rest-${format.name}.json').readAsStringSync(),
        );
        final transport = RecordingTransport();
        await runRestOps(
          DartVerifyCiRestCratestackClient(
            _adapter(transport, format: format),
            basePath: '/api',
          ),
        );
        expect(
          transport.requests.map(_normalize).toList(),
          equals(golden),
        );
      });
    }

    test('the canonical query equals what Dio encodes, for every value kind '
        'the generated query builders produce', () {
      const queries = <Map<String, Object?>>[
        {'limit': 20, 'offset': 0},
        {'sort': '-id', 'where': 'published=true', 'or': 'a=1|b=2'},
        {'fields': 'id,title', 'includeFields[author]': 'id,name'},
        {'q': 'x y&z', 'u': 'ü/é +&=', 'empty': '', 'flag': true},
        {'tag': ['a', 'b c'], 'n': 3},
        {
          'filter': {'kind': 'x y', 'deep': 1},
        },
      ];
      for (final query in queries) {
        expect(
          cratestackCanonicalizeQuery(query),
          Transformer.urlEncodeQueryMap(query),
          reason: '$query',
        );
      }
    });

    test('a null query value is omitted, not sent as a bare key', () {
      expect(cratestackCanonicalizeQuery({'a': 1, 'b': null}), 'a=1');
      expect(cratestackCanonicalizeQuery({'b': null}), isNull);
      expect(cratestackCanonicalizeQuery(const {}), isNull);
      expect(cratestackCanonicalizeQuery(null), isNull);
    });
  });

  group('request construction', () {
    test('idempotencyKey is sent as Idempotency-Key on a REST call', () async {
      final transport = RecordingTransport();
      await _adapter(transport).execute(
        const CratestackRequest(method: 'POST', path: '/api/posts', body: {'a': 1}),
        options: const CratestackCallOptions(idempotencyKey: 'key-1'),
      );
      expect(transport.requests.single.headers['Idempotency-Key'], 'key-1');
    });

    test('call headers win over adapter headers regardless of case', () async {
      final transport = RecordingTransport();
      await _adapter(transport).execute(
        const CratestackRequest(method: 'GET', path: '/api/posts'),
        options: const CratestackCallOptions(headers: {'accept': 'text/plain'}),
      );
      final headers = transport.requests.single.headers;
      expect(headers['accept'], 'text/plain');
      expect(headers.keys.where((k) => k.toLowerCase() == 'accept'), hasLength(1));
    });

    test('a base URI path prefix is kept', () async {
      final transport = RecordingTransport();
      await CratestackHttpAdapter(
        transport,
        baseUri: Uri.parse('https://example.com/v1/'),
      ).execute(
        const CratestackRequest(
          method: 'GET',
          path: '/api/posts',
          queryParameters: {'limit': 1},
        ),
      );
      expect(
        transport.requests.single.uri.toString(),
        'https://example.com/v1/api/posts?limit=1',
      );
    });

    test('useRustTransport travels in extra, never on the wire', () async {
      final transport = RecordingTransport();
      await CratestackHttpAdapter(
        transport,
        baseUri: Uri.parse('http://127.0.0.1:9'),
        useRustTransport: true,
      ).execute(const CratestackRequest(method: 'GET', path: '/api/posts'));
      final request = transport.requests.single;
      expect(request.extra[cratestackUseRustTransportExtraKey], isTrue);
      expect(request.headers.keys, isNot(contains(cratestackUseRustTransportExtraKey)));
    });

    test('abort is handed to the transport', () async {
      final transport = RecordingTransport();
      final abort = Completer<void>();
      await _adapter(transport).execute(
        const CratestackRequest(method: 'GET', path: '/api/posts'),
        options: CratestackCallOptions(abort: abort.future),
      );
      expect(transport.requests.single.abort, same(abort.future));
    });
  });

  group('responses', () {
    test('a CBOR answer decodes', () async {
      final adapter = _adapter(
        RecordingTransport(
          (_) => _answer(
            200,
            'application/cbor; charset=binary',
            cbor.cbor.encode({'id': 1, 'tags': ['a'], 'nested': {'k': true}}),
          ),
        ),
      );
      expect(
        await adapter.execute(const CratestackRequest(method: 'GET', path: '/x')),
        {'id': 1, 'tags': ['a'], 'nested': {'k': true}},
      );
    });

    test('a JSON answer decodes, through the CBOR-format adapter too', () async {
      final adapter = _adapter(
        RecordingTransport(
          (_) => _answer(200, 'application/json', utf8.encode('{"id":1}')),
        ),
      );
      expect(
        await adapter.execute(const CratestackRequest(method: 'GET', path: '/x')),
        {'id': 1},
      );
    });

    test('an empty 2xx body is null', () async {
      final adapter = _adapter(RecordingTransport());
      expect(
        await adapter.execute(const CratestackRequest(method: 'GET', path: '/x')),
        isNull,
      );
    });

    test('a 2xx answer of another Content-Type is a FormatException', () async {
      final adapter = _adapter(
        RecordingTransport((_) => _answer(200, 'text/html', utf8.encode('<p>'))),
      );
      await expectLater(
        adapter.execute(const CratestackRequest(method: 'GET', path: '/x')),
        throwsFormatException,
      );
    });
  });

  group('errors', () {
    Future<Object> failure(CratestackHttpResponse response) async {
      try {
        await _adapter(RecordingTransport((_) => response)).execute(
          const CratestackRequest(method: 'GET', path: '/x'),
        );
      } catch (error) {
        return error;
      }
      fail('expected the call to throw');
    }

    test('a CBOR error body becomes a typed CratestackRestException', () async {
      final error = await failure(
        _answer(409, 'application/cbor', cbor.cbor.encode({'code': 'conflict', 'n': 2})),
      );
      expect(
        error,
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 409)
            .having((e) => e.body, 'body', {'code': 'conflict', 'n': 2}),
      );
    });

    test('a JSON error body becomes a typed CratestackRestException', () async {
      final error = await failure(
        _answer(422, 'application/json', utf8.encode('{"message":"bad"}')),
      );
      expect(
        error,
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 422)
            .having((e) => e.body, 'body', {'message': 'bad'}),
      );
    });

    test('a plain-text or empty error body does not hide the status', () async {
      final text = await failure(_answer(502, 'text/plain', utf8.encode('Bad Gateway')));
      expect(
        text,
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 502)
            .having((e) => e.body, 'body', 'Bad Gateway'),
      );
      final empty = await failure(CratestackHttpResponse.buffered(status: 500));
      expect(
        empty,
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 500)
            .having((e) => e.body, 'body', isNull),
      );
    });

    test('a transport failure passes through untouched', () async {
      const unsent = CratestackTransportUnsent(message: 'refused');
      await expectLater(
        _adapter(_ThrowingTransport(unsent)).execute(
          const CratestackRequest(method: 'GET', path: '/x'),
        ),
        throwsA(same(unsent)),
      );
    });
  });

  group('through CratestackDioTransport', () {
    CratestackHttpAdapter dioAdapter(HttpServer server, {Dio? dio}) =>
        CratestackHttpAdapter(
          CratestackDioTransport(dio ?? Dio()),
          baseUri: Uri.parse('http://127.0.0.1:${server.port}'),
        );

    test('a 5xx is a CratestackRestException, never a DioException', () async {
      final server = await _serve((request) async {
        request.response.statusCode = 503;
        request.response.headers.set('content-type', 'application/json');
        request.response.write('{"message":"down"}');
        await request.response.close();
      });
      await expectLater(
        dioAdapter(server).execute(
          const CratestackRequest(method: 'GET', path: '/x'),
        ),
        throwsA(
          isA<CratestackRestException>()
              .having((e) => e.status, 'status', 503)
              .having((e) => e.body, 'body', {'message': 'down'}),
        ),
      );
    });

    test('a refused connection is CratestackTransportUnsent', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      final adapter = dioAdapter(server);
      await server.close(force: true);
      await expectLater(
        adapter.execute(const CratestackRequest(method: 'GET', path: '/x')),
        throwsA(isA<CratestackTransportUnsent>()),
      );
    });

    test('a receive timeout is CratestackTransportTimedOut', () async {
      final server = await _serve((request) async {
        // Never answers.
      });
      final dio = Dio(BaseOptions(receiveTimeout: const Duration(milliseconds: 200)));
      await expectLater(
        dioAdapter(server, dio: dio).execute(
          const CratestackRequest(method: 'GET', path: '/x'),
        ),
        throwsA(isA<CratestackTransportTimedOut>()),
      );
    });

    test('abort cancels the send: the call fails Aborted and the server '
        'sees the connection drop', () async {
      final requestSeen = Completer<void>();
      final connectionClosed = Completer<void>();
      final server = await _serve((request) async {
        // Take over the socket so the server can tell when the client hangs up.
        final socket = await request.response.detachSocket(writeHeaders: false);
        socket.listen(
          (_) {},
          onError: (Object _) {},
          onDone: () {
            if (!connectionClosed.isCompleted) connectionClosed.complete();
          },
        );
        requestSeen.complete();
      });
      final abort = Completer<void>();
      final call = dioAdapter(server).execute(
        const CratestackRequest(method: 'GET', path: '/x'),
        options: CratestackCallOptions(abort: abort.future),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await requestSeen.future;
      abort.complete();
      await outcome;
      await connectionClosed.future.timeout(const Duration(seconds: 5));
    });

    test('the bytes Dio sends are the bytes the adapter encoded', () async {
      final received = <int>[];
      String? contentType;
      final server = await _serve((request) async {
        contentType = request.headers.contentType?.mimeType;
        await for (final chunk in request) {
          received.addAll(chunk);
        }
        request.response.statusCode = 204;
        await request.response.close();
      });
      await dioAdapter(server).execute(
        const CratestackRequest(
          method: 'POST',
          path: '/x',
          body: {'title': 'Hello, wörld'},
        ),
      );
      expect(contentType, 'application/cbor');
      expect(
        cratestackNormalizeWire(cbor.cbor.decode(Uint8List.fromList(received))),
        {'title': 'Hello, wörld'},
      );
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
