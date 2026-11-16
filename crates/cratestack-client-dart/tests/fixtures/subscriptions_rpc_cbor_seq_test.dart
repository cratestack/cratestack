// cratestack#1001: a real, running proof of the generated Dart client's
// `application/cbor-seq` subscription transport, against a
// `CratestackHttpTransport` that answers with CBOR frames written by hand:
// nothing before `ready`, decoded items, the keep-alive `null` skipped, frames
// split at every byte, `end`, a terminal `err`, a refusal before the stream,
// a body that stops short, and the per-frame byte cap. Run by
// `just verify-dart`, with the real `cratestack_cbor` codec.
import 'dart:convert';
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

/// One CBOR item for the few shapes a frame has.
Uint8List _cbor(Object? value) {
  final out = BytesBuilder();
  void head(int major, int n) {
    if (n < 24) {
      out.addByte(major << 5 | n);
    } else if (n < 256) {
      out.add(<int>[major << 5 | 24, n]);
    } else {
      out.add(<int>[major << 5 | 25, n >> 8, n & 0xff]);
    }
  }

  void write(Object? v) {
    switch (v) {
      case null:
        out.addByte(0xf6);
      case int():
        head(0, v);
      case String():
        final bytes = utf8.encode(v);
        head(3, bytes.length);
        out.add(bytes);
      case Map<String, Object?>():
        head(5, v.length);
        for (final entry in v.entries) {
          write(entry.key);
          write(entry.value);
        }
      default:
        throw ArgumentError('unsupported $v');
    }
  }

  write(value);
  return out.toBytes();
}

Uint8List _join(List<Uint8List> parts) =>
    Uint8List.fromList(<int>[for (final part in parts) ...part]);

final _keepalive = Uint8List.fromList(<int>[0xf6]);
final _ready = _cbor(<String, Object?>{'id': 1, 'ready': <String, Object?>{}});
Uint8List _next(int id, String body) => _cbor(<String, Object?>{
      'id': id,
      'next': <String, Object?>{'body': body},
    });
Uint8List _end(int id) => _cbor(<String, Object?>{'id': id, 'end': <String, Object?>{}});

class _CborSeqTransport implements CratestackHttpTransport {
  _CborSeqTransport(this.status, this.chunks);

  final int status;
  final List<Uint8List> chunks;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: status,
      headers: <String, String>{
        'content-type': status == 200 ? 'application/cbor-seq' : 'application/json',
      },
      body: Stream<Uint8List>.fromIterable(chunks),
    );
  }
}

Future<List<Line>> _lines(
  _CborSeqTransport transport, {
  int cap = cratestackDefaultMaxEventBytes,
  void Function()? onReady,
}) {
  final client = DartVerifySubscriptionsRpcCratestackClient(
    CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
    subscriptions: CratestackSubscriptionOptions(
      transport: CratestackSubscriptionTransport.cborSeq,
      maxEventBytes: cap,
    ),
  );
  return client.procedures
      .lines(const LinesArgs(args: Room(roomId: 'r1')), onReady: onReady)
      .toList();
}

void main() {
  test('asks for the media type, gates on ready, skips keep-alives, ends on end', () async {
    final transport = _CborSeqTransport(200, <Uint8List>[
      _join(<Uint8List>[_keepalive, _ready, _keepalive, _next(2, 'hi'), _end(3)]),
    ]);
    var ready = false;
    final lines = await _lines(transport, onReady: () => ready = true);
    expect(ready, isTrue);
    expect(lines.map((line) => line.body), <String>['hi']);
    final request = transport.requests.single;
    expect(request.method, 'GET');
    expect(
      request.uri.toString(),
      'https://api.test/rpc/subscribe/procedure.lines?args=%7B%22roomId%22%3A%22r1%22%7D',
    );
    expect(request.headers['Accept'], 'application/cbor-seq');
  });

  test('reads frames split at every byte position', () async {
    final wire = _join(<Uint8List>[_ready, _keepalive, _next(2, 'café'), _next(3, 'b'), _end(4)]);
    for (var at = 1; at < wire.length; at++) {
      final transport = _CborSeqTransport(200, <Uint8List>[
        Uint8List.sublistView(wire, 0, at),
        Uint8List.sublistView(wire, at),
      ]);
      final lines = await _lines(transport);
      expect(lines.map((line) => line.body), <String>['café', 'b'], reason: 'split at byte $at');
    }
  });

  test('a terminal err frame is a CratestackSubscriptionException, and nothing follows it', () async {
    final transport = _CborSeqTransport(200, <Uint8List>[
      _join(<Uint8List>[
        _ready,
        _cbor(<String, Object?>{
          'id': 2,
          'err': <String, Object?>{'code': 'unavailable', 'message': 'subscription lagged'},
        }),
        _next(3, 'after'),
      ]),
    ]);
    await expectLater(
      _lines(transport),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'unavailable')
            .having((error) => error.message, 'message', 'subscription lagged'),
      ),
    );
  });

  test('a refusal before the stream carries its status', () async {
    final transport = _CborSeqTransport(403, <Uint8List>[
      Uint8List.fromList(utf8.encode('{"code":"permission_denied","message":"not a member"}')),
    ]);
    await expectLater(
      _lines(transport),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'permission_denied')
            .having((error) => error.status, 'status', 403),
      ),
    );
  });

  test('a body that stops short is an error, not a quiet end', () async {
    final whole = _join(<Uint8List>[_ready, _next(2, 'x' * 40)]);
    await expectLater(
      _lines(_CborSeqTransport(200, <Uint8List>[Uint8List.sublistView(whole, 0, whole.length - 5)])),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.message, 'message', contains('middle of a frame')),
      ),
    );
    await expectLater(
      _lines(_CborSeqTransport(200, <Uint8List>[whole])),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.message, 'message', contains('without an `end` or `err` frame')),
      ),
    );
  });

  test('one frame is capped at maxEventBytes: exactly at the cap passes, one over throws', () async {
    final big = _next(2, 'x' * 300);
    final atCap = await _lines(
      _CborSeqTransport(200, <Uint8List>[_join(<Uint8List>[_ready, big, _end(3)])]),
      cap: big.length,
    );
    expect(atCap, hasLength(1));
    await expectLater(
      _lines(
        _CborSeqTransport(200, <Uint8List>[_join(<Uint8List>[_ready, big, _next(3, 'after')])]),
        cap: big.length - 1,
      ),
      throwsA(
        isA<CratestackEventTooLargeException>().having((error) => error.limit, 'limit', big.length - 1),
      ),
    );
  });
}
