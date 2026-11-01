// Slice S6: a real, running proof of the generated Dart client's
// subscriptions over SSE, against a `CratestackHttpTransport` that answers
// with an event stream: nothing before `Ready`, revived items, a natural end,
// the request the server reads (path, query, `Accept`), a refusal before the
// stream and a terminal error, both as `CratestackSubscriptionException`.
import 'dart:convert';
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

class _SseTransport implements CratestackHttpTransport {
  _SseTransport(this.status, this.chunks);

  final int status;
  final List<String> chunks;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: status,
      headers: <String, String>{
        'content-type': status == 200 ? 'text/event-stream' : 'application/json',
      },
      body: Stream<Uint8List>.fromIterable(
        chunks.map((chunk) => Uint8List.fromList(utf8.encode(chunk))),
      ),
    );
  }
}

DartVerifySubscriptionsRpcCratestackClient _client(_SseTransport transport) {
  return DartVerifySubscriptionsRpcCratestackClient(
    CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
  );
}

void main() {
  test('a subscription procedure emits after Ready and ends with the stream', () async {
    final transport = _SseTransport(200, <String>[
      'event: ready\ndata: {"id":1,"ready":{}}\n\n',
      ': keepalive\n\n',
      'event: message\ndata: {"id":2,"ne',
      'xt":{"body":"hi"}}\n\n',
    ]);
    var ready = false;
    final lines = await _client(transport)
        .procedures
        .lines(const LinesArgs(args: Room(roomId: 'r1')), onReady: () => ready = true)
        .toList();
    expect(ready, isTrue);
    expect(lines.map((line) => line.body), <String>['hi']);
    final request = transport.requests.single;
    expect(request.method, 'GET');
    expect(
      request.uri.toString(),
      'https://api.test/rpc/subscribe/procedure.lines?args=%7B%22roomId%22%3A%22r1%22%7D',
    );
    expect(request.headers['Accept'], 'text/event-stream');
  });

  test('a model feed decodes its events and carries list filters', () async {
    final transport = _SseTransport(200, <String>[
      'event: ready\ndata: {"id":1,"ready":{}}\n\n',
      'event: message\ndata: {"id":2,"next":{"event_id":"e1","model":"Note",'
          '"operation":"Created","occurred_at":"2026-11-01T00:00:00Z",'
          '"data":{"id":7,"body":"mine"}}}\n\n',
    ]);
    final events = await _client(transport)
        .notes
        .subscribe(
          input: const CratestackSubscribeInput(
            filters: <String, String>{'body__contains': 'mi'},
            operations: <String>['created'],
          ),
        )
        .toList();
    expect(events.single.operation, 'Created');
    expect(events.single.data.id, 7);
    expect(events.single.data.body, 'mine');
    expect(
      transport.requests.single.uri.query,
      'body__contains=mi&operations=created',
    );
  });

  test('a refusal and a terminal error are CratestackSubscriptionException', () async {
    final refused = _SseTransport(403, <String>[
      '{"code":"permission_denied","message":"not a member"}',
    ]);
    await expectLater(
      _client(refused).procedures.lines(const LinesArgs(args: Room(roomId: 'r1'))).toList(),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'permission_denied')
            .having((error) => error.status, 'status', 403),
      ),
    );
    final lagged = _SseTransport(200, <String>[
      'event: ready\ndata: {"id":1,"ready":{}}\n\n',
      'event: error\ndata: {"id":2,"err":{"code":"unavailable","message":"subscription lagged"}}\n\n',
    ]);
    await expectLater(
      _client(lagged).procedures.lines(const LinesArgs(args: Room(roomId: 'r1'))).toList(),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'unavailable'),
      ),
    );
  });
}
