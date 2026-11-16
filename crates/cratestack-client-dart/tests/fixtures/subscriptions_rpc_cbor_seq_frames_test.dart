// cratestack#1001: the Dart `application/cbor-seq` transport against a body
// captured from the real Postgres-backed server (a model event's byte-string
// `event_id` and `Uuid`), and its frame validation: a body that stops short,
// the per-frame byte cap, a frame that is not a map, a first frame that is
// not `ready`. The happy path is in `subscriptions_rpc_cbor_seq_test.dart`.
// Run by `just verify-dart`, with the real `cratestack_cbor` codec.
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

import 'subscriptions_rpc_cbor_seq_support.dart';

void main() {
  test('a body that stops short is an error, not a quiet end', () async {
    final whole = seqJoin(<Uint8List>[seqReady, seqNext(2, 'x' * 40)]);
    await expectLater(
      seqLines(CborSeqTransport(200, <Uint8List>[Uint8List.sublistView(whole, 0, whole.length - 5)])),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.message, 'message', contains('middle of a frame')),
      ),
    );
    await expectLater(
      seqLines(CborSeqTransport(200, <Uint8List>[whole])),
      throwsA(
        isA<CratestackSubscriptionException>()
            .having((error) => error.message, 'message', contains('without an `end` or `err` frame')),
      ),
    );
  });

  test('one frame is capped at maxEventBytes: exactly at the cap passes, one over throws', () async {
    final big = seqNext(2, 'x' * 300);
    final atCap = await seqLines(
      CborSeqTransport(200, <Uint8List>[seqJoin(<Uint8List>[seqReady, big, seqEnd(3)])]),
      cap: big.length,
    );
    expect(atCap, hasLength(1));
    await expectLater(
      seqLines(
        CborSeqTransport(200, <Uint8List>[seqJoin(<Uint8List>[seqReady, big, seqNext(3, 'after')])]),
        cap: big.length - 1,
      ),
      throwsA(
        isA<CratestackEventTooLargeException>().having((error) => error.limit, 'limit', big.length - 1),
      ),
    );
  });

  test("a model event's byte-string event_id and Uuid are read as the real server writes them", () async {
    // Captured from the Postgres-backed server: `event_id` and `data.id` are
    // 16-byte CBOR byte strings (a list of 16 integers through the bridge);
    // `amount` is text, which `SeqNote` does not read.
    const hex =
    'a262696401657265616479a0a262696402646e657874a56464617461a366616d6f756e74'
    '6b31322e353030303030303064626f64796568656c6c6f626964500123456789abcdef01'
    '23456789abcdef686576656e745f6964503143e143551549fa8e229f24d82d0a03656d6f'
    '64656c675365714e6f74656b6f636375727265645f6174781e323032362d31312d313654'
    '30343a30393a35342e3332393534363231385a696f7065726174696f6e67437265617465'
    '64';
    final body = Uint8List.fromList(<int>[
      for (var i = 0; i < hex.length; i += 2) int.parse(hex.substring(i, i + 2), radix: 16),
    ]);
    final events = <CratestackModelEvent<SeqNote>>[];
    Object? error;
    try {
      await for (final event in seqClient(CborSeqTransport(200, <Uint8List>[body]), cratestackDefaultMaxEventBytes)
          .seqNotes
          .subscribe()) {
        events.add(event);
      }
    } on CratestackSubscriptionException catch (caught) {
      error = caught;
    }
    expect(events, hasLength(1));
    expect(events.single.eventId, '3143e143-5515-49fa-8e22-9f24d82d0a03');
    expect(events.single.model, 'SeqNote');
    expect(events.single.operation, 'Created');
    expect(events.single.data.id, '01234567-89ab-cdef-0123-456789abcdef');
    expect(events.single.data.body, 'hello');
    // The captured body stops without an `end`, which is an error.
    expect(error, isA<CratestackSubscriptionException>());
  });

  test('a frame that is not a map is a CratestackSubscriptionException', () async {
    for (final bad in <Uint8List>[seqCbor(5), seqCbor('x')]) {
      await expectLater(
        seqLines(CborSeqTransport(200, <Uint8List>[seqJoin(<Uint8List>[seqReady, bad])])),
        throwsA(
          isA<CratestackSubscriptionException>()
              .having((error) => error.code, 'code', 'invalid_response'),
        ),
      );
    }
  });

  test('a stream whose first frame is not ready is refused, and nothing is delivered', () async {
    for (final first in <Uint8List>[seqNext(1, 'early'), seqEnd(1)]) {
      final delivered = <Line>[];
      var ready = false;
      final transport = CborSeqTransport(200, <Uint8List>[seqJoin(<Uint8List>[first, seqReady])]);
      await expectLater(
        seqClient(transport, cratestackDefaultMaxEventBytes)
            .procedures
            .lines(const LinesArgs(args: Room(roomId: 'r1')), onReady: () => ready = true)
            .forEach(delivered.add),
        throwsA(
          isA<CratestackSubscriptionException>()
              .having((error) => error.code, 'code', 'invalid_response')
              .having((error) => error.message, 'message', contains('`ready`')),
        ),
      );
      expect(delivered, isEmpty);
      expect(ready, isFalse);
    }
    // An `err` first is the server's refusal, not a framing fault.
    await expectLater(
      seqLines(
        CborSeqTransport(200, <Uint8List>[
          seqCbor(<String, Object?>{
            'id': 1,
            'err': <String, Object?>{'code': 'permission_denied', 'message': 'no'},
          }),
        ]),
      ),
      throwsA(
        isA<CratestackSubscriptionException>().having((error) => error.code, 'code', 'permission_denied'),
      ),
    );
  });
}
