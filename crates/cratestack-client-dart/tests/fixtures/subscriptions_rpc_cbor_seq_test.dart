// cratestack#1001: a real, running proof of the generated Dart client's
// `application/cbor-seq` subscription transport, against a
// `CratestackHttpTransport` that answers with CBOR frames: a stream opens
// with `ready`, keep-alive `null`s are skipped, frames split at every byte,
// `end`, a terminal `err`, a refusal before the stream and a response that is
// not `application/cbor-seq`. The model event, frame validation and caps are
// in `subscriptions_rpc_cbor_seq_frames_test.dart`. Run by `just verify-dart`,
// with the real `cratestack_cbor` codec.
import 'dart:convert';
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

import 'subscriptions_rpc_cbor_seq_support.dart';

void main() {
  test('asks for the media type, gates on ready, skips keep-alives, ends on end', () async {
    final transport = CborSeqTransport(200, <Uint8List>[
      seqJoin(<Uint8List>[seqKeepalive, seqReady, seqKeepalive, seqNext(2, 'hi'), seqEnd(3)]),
    ]);
    var ready = false;
    final lines = await seqLines(transport, onReady: () => ready = true);
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
    final wire = seqJoin(<Uint8List>[seqReady, seqKeepalive, seqNext(2, 'café'), seqNext(3, 'b'), seqEnd(4)]);
    for (var at = 1; at < wire.length; at++) {
      final transport = CborSeqTransport(200, <Uint8List>[
        Uint8List.sublistView(wire, 0, at),
        Uint8List.sublistView(wire, at),
      ]);
      final lines = await seqLines(transport);
      expect(lines.map((line) => line.body), <String>['café', 'b'], reason: 'split at byte $at');
    }
  });

  test('a terminal err frame is a CratestackSubscriptionException, and nothing follows it', () async {
    final transport = CborSeqTransport(200, <Uint8List>[
      seqJoin(<Uint8List>[
        seqReady,
        seqCbor(<String, Object?>{
          'id': 2,
          'err': <String, Object?>{'code': 'unavailable', 'message': 'subscription lagged'},
        }),
        seqNext(3, 'after'),
      ]),
    ]);
    await expectLater(
      seqLines(transport),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'unavailable')
            .having((error) => error.message, 'message', 'subscription lagged'),
      ),
    );
  });

  test('a refusal before the stream carries its status', () async {
    final transport = CborSeqTransport(403, <Uint8List>[
      Uint8List.fromList(utf8.encode('{"code":"permission_denied","message":"not a member"}')),
    ]);
    await expectLater(
      seqLines(transport),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'permission_denied')
            .having((error) => error.status, 'status', 403),
      ),
    );
  });

  test('a response that is not application/cbor-seq is refused by its media type', () async {
    final sse = Uint8List.fromList(utf8.encode('event: ready\ndata: {"id":1,"ready":{}}\n\n'));
    await expectLater(
      seqLines(CborSeqTransport(200, <Uint8List>[sse], contentType: 'text/event-stream')),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.code, 'code', 'invalid_response')
            .having((error) => error.message, 'message', contains('text/event-stream')),
      ),
    );
    // Parameters and case do not matter.
    final lines = await seqLines(
      CborSeqTransport(
        200,
        <Uint8List>[seqJoin(<Uint8List>[seqReady, seqEnd(2)])],
        contentType: 'Application/CBOR-Seq; charset=binary',
      ),
    );
    expect(lines, isEmpty);
  });
}
