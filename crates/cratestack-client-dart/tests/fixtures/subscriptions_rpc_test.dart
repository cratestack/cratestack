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
  _SseTransport(this.status, List<String> chunks)
      : byteChunks = chunks.map(utf8.encode).toList();

  /// Chunks cut at arbitrary bytes, even inside a multi-byte character.
  _SseTransport.bytes(this.status, this.byteChunks);

  final int status;
  final List<List<int>> byteChunks;
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
        byteChunks.map(Uint8List.fromList),
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
      'event: message\ndata: {"id":3,"next":{"event_id":"e2","model":"Note",'
          '"operation":"Updated","occurred_at":"2026-11-01T00:00:01Z",'
          '"matches":false,"data":{"id":7,"body":"mine"}}}\n\n',
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
    expect(events.first.operation, 'Created');
    expect(events.first.matches, isTrue, reason: 'absent on the wire means a match');
    expect(events.first.data.id, 7);
    expect(events.first.data.body, 'mine');
    // A filter-exit event: the row left the filter, `data` is what it held.
    expect(events.last.operation, 'Updated');
    expect(events.last.matches, isFalse);
    expect(events.last.data.body, 'mine');
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

  test('endReason reads details.reason; the retry rule follows it (cratestack#1381)', () async {
    Future<CratestackSubscriptionException> endedWith(String body) async {
      final transport = _SseTransport(200, <String>[
        'event: ready\ndata: {"id":1,"ready":{}}\n\n',
        'event: error\ndata: {"id":2,"err":$body}\n\n',
      ]);
      try {
        await _client(transport)
            .procedures
            .lines(const LinesArgs(args: Room(roomId: 'r1')))
            .toList();
      } on CratestackSubscriptionException catch (error) {
        return error;
      }
      fail('the stream should have ended with an error');
    }

    const cases = <String, List<Object>>{
      'credential_expired': ['unauthenticated', CratestackStreamEndReason.credentialExpired],
      'stream_lifetime': ['unavailable', CratestackStreamEndReason.streamLifetime],
      'session_ended': ['unauthenticated', CratestackStreamEndReason.sessionEnded],
      'principal_suspended': ['permission_denied', CratestackStreamEndReason.principalSuspended],
      'device_revoked': ['permission_denied', CratestackStreamEndReason.deviceRevoked],
      'revocation_resync': ['unavailable', CratestackStreamEndReason.revocationResync],
    };
    for (final entry in cases.entries) {
      final error = await endedWith(
        '{"code":"${entry.value[0]}","message":"ended","details":{"reason":"${entry.key}"}}',
      );
      expect(error.endReason, entry.value[1], reason: entry.key);
    }
    expect(CratestackStreamEndReason.sessionEnded.isTerminal, isTrue);
    expect(CratestackStreamEndReason.principalSuspended.isTerminal, isTrue);
    expect(CratestackStreamEndReason.deviceRevoked.isTerminal, isTrue);
    expect(CratestackStreamEndReason.credentialExpired.isTerminal, isFalse);
    expect(CratestackStreamEndReason.streamLifetime.resubscribes, isTrue);
    expect(CratestackStreamEndReason.revocationResync.resubscribes, isTrue);
    expect(CratestackStreamEndReason.credentialExpired.resubscribes, isFalse);

    final unknown = await endedWith(
      '{"code":"unavailable","message":"ended","details":{"reason":"from_the_future"}}',
    );
    expect(unknown.endReason, CratestackStreamEndReason.unknown);
    expect(unknown.code, 'unavailable');
    final bare = await endedWith('{"code":"unavailable","message":"ended"}');
    expect(bare.endReason, isNull);
    final odd = await endedWith('{"code":"unavailable","message":"ended","details":"x"}');
    expect(odd.endReason, isNull);
  });

  group('the per-event byte cap', () {
    const limit = 200;
    const ready = 'event: ready\ndata: {"id":1,"ready":{}}\n\n';
    const good = 'event: message\ndata: {"id":2,"next":{"body":"ok"}}\n\n';

    Future<List<Line>> drain(
      List<String> chunks, {
      int cap = limit,
      _SseTransport? over,
    }) {
      final transport = over ?? _SseTransport(200, chunks);
      final client = DartVerifySubscriptionsRpcCratestackClient(
        CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
        subscriptions: CratestackSubscriptionOptions(
          transport: CratestackSubscriptionTransport.sse,
          maxEventBytes: cap,
        ),
      );
      return client.procedures
          .lines(const LinesArgs(args: Room(roomId: 'r1')))
          .toList();
    }

    // A comment-only block `size` bytes long, its `\n\n` included.
    String comment(int size) => ': ${'x' * (size - 4)}\n\n';

    test('an event exactly at the cap is accepted', () async {
      final lines = await drain(<String>[ready, comment(limit), good]);
      expect(lines.map((line) => line.body), <String>['ok']);
    });

    test('an event one byte over the cap is refused and nothing follows', () async {
      await expectLater(
        drain(<String>[ready, comment(limit + 1) + good]),
        throwsA(
          isA<CratestackEventTooLargeException>()
              .having((error) => error.limit, 'limit', limit)
              .having((error) => error.code, 'code', 'event_too_large'),
        ),
      );
    });

    test('an event with no delimiter ends with the error', () async {
      await expectLater(
        drain(<String>[ready, ...List<String>.filled(50, 'x' * 100)]),
        throwsA(isA<CratestackEventTooLargeException>()),
      );
    });

    test('the request is cancelled once an event is too large', () async {
      final transport = _SseTransport(200, <String>[
        ready,
        comment(limit + 1),
      ]);
      await expectLater(
        drain(const <String>[], over: transport),
        throwsA(isA<CratestackEventTooLargeException>()),
      );
      await expectLater(transport.requests.single.abort!, completes);
    });

    test('a ready event over the cap is the same typed error', () async {
      await expectLater(
        drain(<String>['event: ready\ndata: ${'x' * limit}\n\n']),
        throwsA(isA<CratestackEventTooLargeException>()),
      );
    });

    test('CRLF events split at every byte position decode exactly', () async {
      final stream = utf8.encode(
        'event: ready\r\ndata: {"id":1,"ready":{}}\r\n\r\n'
        'event: message\r\ndata: {"id":2,"next":{"body":"caf\u00e9"}}\r\n\r\n'
        'event: message\r\ndata: {"id":3,"next":{"body":"b"}}\r\n\r\n',
      );
      for (var at = 1; at < stream.length; at++) {
        final lines = await drain(
          const <String>[],
          cap: 1 << 20,
          over: _SseTransport.bytes(200, <List<int>>[
            stream.sublist(0, at),
            stream.sublist(at),
          ]),
        );
        expect(
          lines.map((line) => line.body),
          <String>['caf\u00e9', 'b'],
          reason: 'split at byte $at',
        );
      }
    });

    test('bytes are counted, not characters, and the default is 1 MiB', () async {
      await expectLater(
        drain(<String>[ready, ': ${'é' * 100}\n\n']),
        throwsA(isA<CratestackEventTooLargeException>()),
      );
      expect(cratestackDefaultMaxEventBytes, 1024 * 1024);
    });
  });
}
