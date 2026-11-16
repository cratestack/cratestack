// cratestack#1378: the generated Dart client's half of a signed subscription
// open, run against a fake `CratestackHttpTransport` (the app's transport is
// what seals; this one only answers). A subscription the schema marks
// `@frames(unsigned)` hands the transport its route and contract in `extra`,
// and accepts only the unsigned stream it asked for; an unmarked one hands
// nothing and refuses a marker. Run by `just verify-dart`, with the real
// `cratestack_cbor` codec.
import 'dart:typed_data';

import 'package:dart_verify_subscriptions_rpc/dart_verify_subscriptions_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

import 'subscriptions_rpc_cbor_seq_support.dart';

const _seq = 'application/cbor-seq';
const _sse = 'text/event-stream';

/// A transport answering every request with the same response, and keeping
/// the requests it saw.
class _Transport implements CratestackHttpTransport {
  _Transport({
    this.status = 200,
    Map<String, String>? headers,
    List<Uint8List>? chunks,
  })  : headers = headers ?? <String, String>{'content-type': _seq, 'cratestack-frames': 'unsigned'},
        chunks = chunks ?? <Uint8List>[seqJoin(<Uint8List>[seqReady, seqNext(2, 'hi'), seqEnd(3)])];

  final int status;
  final Map<String, String> headers;
  final List<Uint8List> chunks;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: status,
      headers: headers,
      body: Stream<Uint8List>.fromIterable(chunks),
    );
  }
}

DartVerifySubscriptionsRpcCratestackClient _client(
  _Transport transport, [
  CratestackSubscriptionTransport kind = CratestackSubscriptionTransport.cborSeq,
]) =>
    DartVerifySubscriptionsRpcCratestackClient(
      CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
      subscriptions: CratestackSubscriptionOptions(transport: kind),
    );

Future<List<Line>> _pulse(_Transport transport,
        [CratestackSubscriptionTransport kind = CratestackSubscriptionTransport.cborSeq]) =>
    _client(transport, kind)
        .procedures
        .pulse(const PulseArgs(args: Room(roomId: 'r1')))
        .toList();

Future<List<Line>> _lines(_Transport transport) => _client(transport)
    .procedures
    .lines(const LinesArgs(args: Room(roomId: 'r1')))
    .toList();

Future<Object?> _failure(Future<Object?> run) async {
  try {
    await run;
  } on Object catch (error) {
    return error;
  }
  return null;
}

void expectRefused(Object? error) {
  expect(error, isA<CratestackSubscriptionException>());
  expect((error as CratestackSubscriptionException).code, 'unsigned_frames_refused');
}

void main() {
  test('a marked open hands the transport its route and contract, and reads the stream', () async {
    final transport = _Transport();
    final lines = await _pulse(transport);
    expect(lines.map((line) => line.body), <String>['hi']);
    final extra = transport.requests.single.extra;
    expect(extra['cratestack.frames'], 'unsigned');
    expect(extra['cratestack.route'], 'subscribe/procedure.pulse');
    expect(extra['cratestack.contract'], cratestackOpContracts['procedure.pulse']);
    expect(extra['cratestack.contract'], isNotNull);
  });

  test('a marked open over SSE hands the same extra and checks the SSE type', () async {
    final sse = Uint8List.fromList(
      'event: ready\ndata: {"id":1,"ready":{}}\n\nevent: message\ndata: {"id":2,"next":{"body":"hi"}}\n\n'.codeUnits,
    );
    final transport = _Transport(
      headers: <String, String>{'content-type': _sse, 'cratestack-frames': 'unsigned'},
      chunks: <Uint8List>[sse],
    );
    final lines = await _pulse(transport, CratestackSubscriptionTransport.sse);
    expect(lines.map((line) => line.body), <String>['hi']);
    expect(transport.requests.single.extra['cratestack.route'], 'subscribe/procedure.pulse');
    // cbor-seq asked for, SSE answered: not the stream that was asked for.
    final wrong = _Transport(
      headers: <String, String>{'content-type': _sse, 'cratestack-frames': 'unsigned'},
      chunks: <Uint8List>[sse],
    );
    expectRefused(await _failure(_pulse(wrong)));
  });

  test('an unmarked open carries none of it', () async {
    final transport = _Transport(
      headers: <String, String>{'content-type': _seq},
    );
    await _lines(transport);
    final extra = transport.requests.single.extra;
    expect(extra.keys.where((key) => key.startsWith('cratestack.')), isEmpty);
  });

  test('a marked answer without the marker, or with another value, is refused', () async {
    for (final marker in <String?>[null, 'signed', 'Unsigned', 'unsigned, unsigned', '']) {
      final transport = _Transport(
        headers: <String, String>{
          'content-type': _seq,
          if (marker != null) 'cratestack-frames': marker,
        },
      );
      expectRefused(await _failure(_pulse(transport)));
    }
  });

  test('a marked answer of another type than was asked for is refused', () async {
    for (final type in <String>['application/cbor', _sse, '']) {
      final transport = _Transport(
        headers: <String, String>{'content-type': type, 'cratestack-frames': 'unsigned'},
      );
      expectRefused(await _failure(_pulse(transport)));
    }
  });

  test('the marker on a subscription the schema does not mark is refused', () async {
    final transport = _Transport();
    expectRefused(await _failure(_lines(transport)));
  });

  test('a refusal before the stream is read as JSON or as the CBOR a sealed refusal opened to', () async {
    final json = _Transport(
      status: 403,
      headers: <String, String>{'content-type': 'application/json'},
      chunks: <Uint8List>[Uint8List.fromList('{"code":"permission_denied","message":"no"}'.codeUnits)],
    );
    final cbor = _Transport(
      status: 403,
      headers: <String, String>{'content-type': 'application/cbor'},
      chunks: <Uint8List>[
        seqCbor(<String, Object?>{'code': 'permission_denied', 'message': 'no'}),
      ],
    );
    for (final transport in <_Transport>[json, cbor]) {
      final error = await _failure(_pulse(transport));
      expect(error, isA<CratestackSubscriptionException>());
      expect((error as CratestackSubscriptionException).code, 'permission_denied');
      expect(error.status, 403);
    }
  });
}
