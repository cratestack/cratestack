// cratestack#1378: the generated Dart client's half of a signed subscription
// open, run against a fake `CratestackHttpTransport` (the app's transport is
// what seals; this one only answers). With the opt-in
// (`CratestackSubscriptionOptions.sealsSubscriptions`), a subscription the
// schema marks `@frames(unsigned)` hands the transport its route and contract
// in `extra` and accepts only the unsigned stream it asked for; an unmarked
// one, and a socket pin, are `streams_unsupported` with nothing sent. Without
// it, nothing changes: no `extra`, no marker check. Run by `just verify-dart`,
// with the real `cratestack_cbor` codec.
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
    this.opened = false,
  })  : headers = headers ?? <String, String>{'content-type': _seq, 'cratestack-frames': 'unsigned'},
        chunks = chunks ?? <Uint8List>[seqJoin(<Uint8List>[seqReady, seqNext(2, 'hi'), seqEnd(3)])];

  final int status;
  final Map<String, String> headers;
  final List<Uint8List> chunks;
  final bool opened;
  final requests = <CratestackHttpRequest>[];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return CratestackHttpResponse(
      status: status,
      headers: headers,
      body: Stream<Uint8List>.fromIterable(chunks),
      opened: opened,
    );
  }
}

/// A client over [transport]; [seals] is the opt-in under test (the sealed
/// cases run with it on, the plain ones with it off).
DartVerifySubscriptionsRpcCratestackClient _client(
  _Transport transport,
  CratestackSubscriptionTransport kind,
  bool seals,
) =>
    DartVerifySubscriptionsRpcCratestackClient(
      CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('https://api.test')),
      subscriptions: CratestackSubscriptionOptions(transport: kind, sealsSubscriptions: seals),
    );

Future<List<Line>> _pulse(
  _Transport transport, {
  CratestackSubscriptionTransport kind = CratestackSubscriptionTransport.cborSeq,
  bool seals = true,
}) =>
    _client(transport, kind, seals)
        .procedures
        .pulse(const PulseArgs(args: Room(roomId: 'r1')))
        .toList();

Future<List<Line>> _lines(
  _Transport transport, {
  CratestackSubscriptionTransport kind = CratestackSubscriptionTransport.cborSeq,
  bool seals = true,
}) =>
    _client(transport, kind, seals)
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

void expectCode(Object? error, String code) {
  expect(error, isA<CratestackSubscriptionException>());
  expect((error as CratestackSubscriptionException).code, code);
}

void expectRefused(Object? error) => expectCode(error, 'unsigned_frames_refused');

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
    final lines = await _pulse(transport, kind: CratestackSubscriptionTransport.sse);
    expect(lines.map((line) => line.body), <String>['hi']);
    expect(transport.requests.single.extra['cratestack.route'], 'subscribe/procedure.pulse');
    // cbor-seq asked for, SSE answered: not the stream that was asked for.
    final wrong = _Transport(
      headers: <String, String>{'content-type': _sse, 'cratestack-frames': 'unsigned'},
      chunks: <Uint8List>[sse],
    );
    expectRefused(await _failure(_pulse(wrong)));
  });

  test('without the opt-in, a marked open is a plain one: no extra, an unmarked stream is read', () async {
    final transport = _Transport(
      headers: <String, String>{'content-type': _seq},
    );
    final lines = await _pulse(transport, seals: false);
    expect(lines.map((line) => line.body), <String>['hi']);
    final extra = transport.requests.single.extra;
    expect(extra.keys.where((key) => key.startsWith('cratestack.')), isEmpty);
    // Nor is a marker, or another type than cbor-seq, checked.
    final marked = _Transport(
      headers: <String, String>{'content-type': _seq, 'cratestack-frames': 'signed'},
    );
    expect((await _pulse(marked, seals: false)).map((line) => line.body), <String>['hi']);
  });

  test('without the opt-in, an unmarked subscription reads its stream as it always did', () async {
    final transport = _Transport(headers: <String, String>{'content-type': _seq});
    await _lines(transport, seals: false);
    final extra = transport.requests.single.extra;
    expect(extra.keys.where((key) => key.startsWith('cratestack.')), isEmpty);
    // Even one the server marked: the marker is not this client's business.
    final marked = _Transport();
    expect((await _lines(marked, seals: false)).map((line) => line.body), <String>['hi']);
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

  test('with the opt-in, an unmarked subscription is streams_unsupported and nothing is sent', () async {
    for (final kind in <CratestackSubscriptionTransport>[
      CratestackSubscriptionTransport.auto,
      CratestackSubscriptionTransport.sse,
      CratestackSubscriptionTransport.cborSeq,
    ]) {
      final transport = _Transport();
      final error = await _failure(_lines(transport, kind: kind));
      expectCode(error, 'streams_unsupported');
      expect(transport.requests, isEmpty);
    }
  });

  test('with the opt-in, a socket pin is streams_unsupported before anything is connected or sent', () async {
    for (final kind in <CratestackSubscriptionTransport>[
      CratestackSubscriptionTransport.webSocket,
      CratestackSubscriptionTransport.webTransport,
    ]) {
      final transport = _Transport();
      expectCode(await _failure(_pulse(transport, kind: kind)), 'streams_unsupported');
      expectCode(await _failure(_lines(transport, kind: kind)), 'streams_unsupported');
      expect(transport.requests, isEmpty);
    }
  });

  test('with the opt-in, auto reads cbor-seq through the transport and never a socket', () async {
    final transport = _Transport();
    final lines = await _pulse(transport, kind: CratestackSubscriptionTransport.auto);
    expect(lines.map((line) => line.body), <String>['hi']);
    final request = transport.requests.single;
    final accept = request.headers.entries.firstWhere((entry) => entry.key.toLowerCase() == 'accept');
    expect(accept.value, _seq);
    expect(request.extra['cratestack.frames'], 'unsigned');
  });

  test('an opened refusal keeps its real code, as JSON or as the CBOR it opened to', () async {
    final json = _Transport(
      status: 403,
      opened: true,
      headers: <String, String>{'content-type': 'application/json'},
      chunks: <Uint8List>[Uint8List.fromList('{"code":"permission_denied","message":"no"}'.codeUnits)],
    );
    final cbor = _Transport(
      status: 403,
      opened: true,
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

  test('a refusal the transport did not open is unsigned_refusal, never the body\'s code', () async {
    for (final kind in <CratestackSubscriptionTransport>[
      CratestackSubscriptionTransport.cborSeq,
      CratestackSubscriptionTransport.sse,
    ]) {
      final transport = _Transport(
        status: 403,
        headers: <String, String>{'content-type': 'application/json'},
        chunks: <Uint8List>[Uint8List.fromList('{"code":"permission_denied","message":"forged"}'.codeUnits)],
      );
      final error = await _failure(_pulse(transport, kind: kind));
      expectCode(error, 'unsigned_refusal');
      expect((error as CratestackSubscriptionException).status, 403);
      expect(error.message, isNot(contains('forged')));
    }
  });

  test('without the opt-in, a refusal is read as JSON or CBOR as it always was', () async {
    final json = _Transport(
      status: 403,
      headers: <String, String>{'content-type': 'application/json'},
      chunks: <Uint8List>[Uint8List.fromList('{"code":"permission_denied","message":"no"}'.codeUnits)],
    );
    final error = await _failure(_pulse(json, seals: false));
    expectCode(error, 'permission_denied');
    expect((error as CratestackSubscriptionException).status, 403);
  });

  test('a refusal body whose code or message is not a string is a typed exception, not a TypeError', () async {
    for (final seals in <bool>[true, false]) {
      final transport = _Transport(
        status: 500,
        opened: true,
        headers: <String, String>{'content-type': 'application/json'},
        chunks: <Uint8List>[Uint8List.fromList('{"code":7,"message":null,"details":1}'.codeUnits)],
      );
      final error = await _failure(_pulse(transport, seals: seals));
      expectCode(error, 'unknown');
      expect((error as CratestackSubscriptionException).message, 'HTTP 500');
      expect(error.status, 500);
    }
    // A body that is not a map at all.
    final listBody = _Transport(
      status: 502,
      opened: true,
      headers: <String, String>{'content-type': 'application/json'},
      chunks: <Uint8List>[Uint8List.fromList('[1,2]'.codeUnits)],
    );
    expectCode(await _failure(_pulse(listBody)), 'unknown');
  });

  test('a terminal err frame with a non-string code is a typed exception', () async {
    final transport = _Transport(
      chunks: <Uint8List>[
        seqJoin(<Uint8List>[seqReady, seqCbor(<String, Object?>{'id': 2, 'err': <String, Object?>{'code': 5, 'message': 6}})]),
      ],
    );
    final error = await _failure(_pulse(transport));
    expectCode(error, 'unknown');
  });
}
