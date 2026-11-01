// Slice S6: the generated Dart client over a WebSocket, against a `dart:io`
// server in this process that speaks the subscription frames (`Init`,
// `Request`, `Ready`, items, `end`/`err`, `cancel`), and the binding choice:
// a pinned socket, `auto` on a socket, `auto` falling back to SSE when no
// socket opens, never on a refusal, and staying on SSE without credentials.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

/// Answers SSE; records each request so a test can see SSE was (not) used.
class _SseTransport implements CratestackHttpTransport {
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: 200,
      headers: const <String, String>{'content-type': 'text/event-stream'},
      body: Stream<Uint8List>.fromIterable(<String>[
        'event: ready\r\ndata: {"id":1,"ready":{}}\r',
        '\n\r\nevent: message\r\ndata: {"id":2,"next":{"body":"over sse"}}\r\n\r\n',
      ].map((chunk) => Uint8List.fromList(utf8.encode(chunk)))),
    );
  }
}

/// One socket per subscription, as the generated client opens them.
class _Server {
  late final HttpServer http;
  final inits = <Map<String, Object?>>[];
  final requests = <Map<String, Object?>>[];
  final cancels = <Object?>[];
  final protocols = <String?>[];

  Future<void> start() async {
    http = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    http.listen((request) async {
      final socket = await WebSocketTransformer.upgrade(
        request,
        protocolSelector: (offered) => offered.first,
      );
      protocols.add(socket.protocol);
      void send(Map<String, Object?> frame) => socket.add(jsonEncode(frame));
      socket.listen((message) {
        final frame = jsonDecode(message as String) as Map<String, Object?>;
        if (frame['init'] != null) {
          final init = frame['init'] as Map<String, Object?>;
          inits.add(init);
          final headers = init['headers'] as Map<String, Object?>;
          if (headers['authorization'] != 'Bearer 1') {
            send({'id': 0, 'err': {'code': 'unauthenticated', 'message': 'no credentials'}});
            socket.close(4401);
            return;
          }
          send({'id': 0, 'ready': <String, Object?>{}});
        } else if (frame['cancel'] != null) {
          cancels.add(frame['id']);
          socket.close();
        } else {
          requests.add(frame);
          final id = frame['id'];
          send({'id': id, 'ready': <String, Object?>{}});
          if (frame['op'] == 'procedure.lines') {
            send({'id': id, 'next': {'body': 'hi'}});
            send({'id': id, 'next': {'body': 'there'}});
            send({'id': id, 'end': <String, Object?>{}});
          } else {
            send({
              'id': id,
              'next': {
                'event_id': 'e1',
                'model': 'Note',
                'operation': 'Created',
                'occurred_at': '2026-11-01T00:00:00Z',
                'data': {'id': 7, 'body': 'live'},
              },
            });
          }
        }
      });
    });
  }

  Uri get url => Uri.parse('ws://127.0.0.1:${http.port}/rpc/ws');
}

DartVerifySubscriptionsRpcCratestackClient _client(
  _SseTransport transport,
  CratestackSubscriptionOptions options,
) {
  return DartVerifySubscriptionsRpcCratestackClient(
    CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
    subscriptions: options,
  );
}

Map<String, String> _asUserOne() => const <String, String>{'authorization': 'Bearer 1'};

const _room = LinesArgs(args: Room(roomId: 'r1'));

void main() {
  late _Server server;
  setUp(() async {
    server = _Server();
    await server.start();
  });
  tearDown(() => server.http.close(force: true));

  test('a pinned WebSocket carries credentials, Ready, items and the end', () async {
    final transport = _SseTransport();
    var ready = false;
    final lines = await _client(
      transport,
      CratestackSubscriptionOptions(
        transport: CratestackSubscriptionTransport.webSocket,
        webSocketUrl: server.url,
        credentials: _asUserOne,
      ),
    ).procedures.lines(_room, onReady: () => ready = true).toList();
    expect(ready, isTrue);
    expect(lines.map((line) => line.body), <String>['hi', 'there']);
    expect(server.protocols.single, 'cratestack.rpc.v1+json');
    expect((server.inits.single['headers'] as Map)['authorization'], 'Bearer 1');
    expect(server.requests.single['op'], 'procedure.lines');
    expect(server.requests.single['input'], {'args': {'roomId': 'r1'}});
    expect(transport.requests, isEmpty);
  });

  test('auto takes the socket; cancelling sends cancel', () async {
    final transport = _SseTransport();
    final events = _client(
      transport,
      CratestackSubscriptionOptions(webSocketUrl: server.url, credentials: _asUserOne),
    ).notes.subscribe(
          input: const CratestackSubscribeInput(
            filters: <String, String>{'body__contains': 'li'},
            operations: <String>['created'],
          ),
        );
    final event = await events.first;
    expect(event.operation, 'Created');
    expect(event.data.body, 'live');
    expect(server.requests.single['input'], {
      'filters': [
        {'key': 'body__contains', 'value': 'li'},
      ],
      'operations': ['created'],
    });
    await Future<void>.delayed(const Duration(milliseconds: 200));
    expect(server.cancels, <Object?>[1]);
    expect(transport.requests, isEmpty);
  });

  test('auto falls back to SSE when no socket opens, across a split CRLF', () async {
    final transport = _SseTransport();
    final closed = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final dead = Uri.parse('ws://127.0.0.1:${closed.port}/rpc/ws');
    await closed.close();
    final lines = await _client(
      transport,
      CratestackSubscriptionOptions(webSocketUrl: dead, credentials: _asUserOne),
    ).procedures.lines(_room).toList();
    expect(lines.map((line) => line.body), <String>['over sse']);
    expect(transport.requests.single.uri.path, '/rpc/subscribe/procedure.lines');
  });

  test('a refusal on the socket is thrown, never a fallback', () async {
    final transport = _SseTransport();
    await expectLater(
      _client(
        transport,
        CratestackSubscriptionOptions(
          webSocketUrl: server.url,
          credentials: () => const <String, String>{'authorization': 'Bearer nope'},
        ),
      ).procedures.lines(_room).toList(),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'unauthenticated'),
      ),
    );
    expect(transport.requests, isEmpty);
  });

  test('without credentials auto stays on SSE', () async {
    final transport = _SseTransport();
    final lines = await _client(
      transport,
      CratestackSubscriptionOptions(webSocketUrl: server.url),
    ).procedures.lines(_room).toList();
    expect(lines.map((line) => line.body), <String>['over sse']);
    expect(server.inits, isEmpty);
  });
}
