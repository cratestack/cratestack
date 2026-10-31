// cratestack#1204, `--preset fespalier` (RPC): the generated
// `CratestackFespalierTransport` over fespalier_transport's `FakeTransport`.
// The RPC twin of `fespalier_transport_rest_test.dart`; see that file for what
// is proved and how `just verify-dart-fespalier` runs it (here against the
// package generated from `ci_rpc.cstack`, library name `dart_verify_ci_rpc`).

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:fespalier_transport/testing.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'wire_parity_rpc_ops.dart';

String _hex(List<int> bytes) =>
    bytes.map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();

Map<String, Object?> _normalize(SentRequest sent) {
  final headers = <String, String>{
    for (final entry in sent.headers.entries)
      entry.key.toLowerCase(): entry.value == cratestackSchemaSha256
          ? '<schema-sha>'
          : entry.value,
  };
  return {
    'method': sent.method,
    'uri': sent.url.toString(),
    'headers': Map.fromEntries(
      headers.entries.toList()..sort((a, b) => a.key.compareTo(b.key)),
    ),
    'bodyHex': _hex(sent.body),
  };
}

CratestackRpcHttpAdapter _adapter(
  HttpTransport transport, {
  CratestackWireFormat format = CratestackWireFormat.cbor,
}) => CratestackRpcHttpAdapter(
  CratestackFespalierTransport(transport),
  baseUri: Uri.parse('http://127.0.0.1:9'),
  format: format,
);

/// A decorator that records the bytes it was handed, like a request signer.
class _Signer implements HttpTransport {
  _Signer(this.inner);

  final HttpTransport inner;
  Uint8List? seen;

  @override
  Future<TransportResponse> send(TransportRequest request) {
    seen = request.body;
    return inner.send(request);
  }
}

TransportResponse _answer(int status, String contentType, List<int> body) =>
    TransportResponse.fromBytes(
      statusCode: status,
      headers: {'content-type': contentType},
      bytes: Uint8List.fromList(body),
    );

Future<Object> _failure(Future<Object?> Function() call) async {
  try {
    await call();
  } catch (error) {
    return error;
  }
  fail('expected the call to throw');
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
  group('wire parity through fespalier_transport', () {
    for (final format in CratestackWireFormat.values) {
      test('${format.name}: every RPC operation sends the golden request', () async {
        final golden = jsonDecode(
          File('test/wire-golden/rpc-${format.name}.json').readAsStringSync(),
        );
        final fake = FakeTransport((_, __) => fakeResponse(204));
        await runRpcOps(
          DartVerifyCiRpcCratestackClient(_adapter(fake, format: format)),
        );
        expect(fake.requests.map(_normalize).toList(), equals(golden));
      });
    }

    test('a decorator sees the exact CBOR bytes the inner transport receives', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      final signer = _Signer(fake);
      await _adapter(signer).call('model.Author.get', {'id': 7});
      final sent = fake.requests.single;
      expect(sent.url.path, '/rpc/model.Author.get');
      expect(sent.lowerHeaders['content-type'], 'application/cbor');
      expect(signer.seen, isNotNull);
      expect(sent.body, orderedEquals(signer.seen!));
      expect(cbor.cbor.decode(sent.body), {'id': 7});
    });

    test('the answer is decoded by its own Content-Type', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, 'application/cbor', cbor.cbor.encode({'id': 1})),
      );
      expect(await _adapter(fake).call('model.Author.get', {'id': 1}), {'id': 1});
    });

    test('cbor-seq frames arrive before the body ends', () async {
      final body = StreamController<List<int>>();
      final fake = FakeTransport(
        (_, __) => TransportResponse(
          statusCode: 200,
          headers: const {'content-type': 'application/cbor-seq'},
          body: body.stream,
        ),
      );
      final items = StreamIterator(_adapter(fake).stream('model.Post.list', {}));
      body.add(_seq([
        {'id': 1},
      ]));
      expect(await items.moveNext().timeout(const Duration(seconds: 5)), isTrue);
      expect(items.current, {'id': 1});
      body.add(_seq([
        {'id': 2},
      ]));
      // Not awaited: the close completes only once the stream has been read.
      unawaited(body.close());
      expect(await items.moveNext(), isTrue);
      expect(items.current, {'id': 2});
      expect(await items.moveNext(), isFalse);
    });
  });

  group('abort', () {
    test('the abort trigger of the current TransportZone aborts the call', () async {
      final fake = FakeTransport((_, __) => Completer<TransportResponse>().future);
      final trigger = Completer<void>();
      final call = TransportZone.run(
        trigger.future,
        () => _adapter(fake).call('x', {}),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await pumpEventQueue();
      expect(fake.requests.single.abortable, isTrue);
      trigger.complete();
      await outcome;
      expect(fake.abortCount, 1);
    });

    test('the call option abort aborts the send, with a zone trigger that never fires', () async {
      final fake = FakeTransport((_, __) => Completer<TransportResponse>().future);
      final zone = Completer<void>();
      final option = Completer<void>();
      final call = TransportZone.run(
        zone.future,
        () => _adapter(fake).call(
          'x',
          {},
          options: CratestackRpcCallOptions(abort: option.future),
        ),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await pumpEventQueue();
      expect(fake.requests.single.abortable, isTrue);
      option.complete();
      await outcome;
      expect(fake.abortCount, 1);
    });

    test('the zone trigger aborts a send that carries its own abort', () async {
      final fake = FakeTransport((_, __) => Completer<TransportResponse>().future);
      final zone = Completer<void>();
      final option = Completer<void>();
      final call = TransportZone.run(
        zone.future,
        () => _adapter(fake).call(
          'x',
          {},
          options: CratestackRpcCallOptions(abort: option.future),
        ),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await pumpEventQueue();
      zone.complete();
      await outcome;
      expect(fake.abortCount, 1);
      expect(option.isCompleted, isFalse);
    });
  });

  group('failures', () {
    Future<Object> failWith(Object thrown) => _failure(
      () => _adapter(FakeTransport((_, __) => throw thrown)).call('x', {}),
    );

    test('Aborted, Unsent, TimedOut and Broken map one to one', () async {
      expect(
        await failWith(const TransportUnsent('refused')),
        isA<CratestackTransportUnsent>().having(
          (e) => e.cause,
          'cause',
          isA<TransportUnsent>(),
        ),
      );
      expect(await failWith(const TransportAborted()), isA<CratestackTransportAborted>());
      expect(await failWith(const TransportTimedOut()), isA<CratestackTransportTimedOut>());
      expect(await failWith(const TransportBroken()), isA<CratestackTransportBroken>());
    });

    test('Portal and Untrusted become Broken and keep the original as cause', () async {
      expect(
        await failWith(const TransportPortal()),
        isA<CratestackTransportBroken>().having((e) => e.cause, 'cause', isA<TransportPortal>()),
      );
      expect(
        await failWith(const TransportUntrusted()),
        isA<CratestackTransportBroken>().having(
          (e) => e.cause,
          'cause',
          isA<TransportUntrusted>(),
        ),
      );
    });

    test('a TransportStatusError is treated as the answer it carries', () async {
      final error = await failWith(
        TransportStatusError(
          statusCode: 400,
          headers: const {'content-type': 'application/cbor'},
          body: Uint8List.fromList(
            cbor.cbor.encode({'code': 'invalid_argument', 'message': 'no'}),
          ),
        ),
      );
      expect(
        error,
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 400)
            .having((e) => e.code, 'code', 'invalid_argument'),
      );
    });

    test('an error answer is a CratestackRpcException, not a transport error', () async {
      final fake = FakeTransport(
        (_, __) => _answer(
          404,
          'application/json',
          utf8.encode('{"code":"not_found","message":"gone"}'),
        ),
      );
      expect(
        await _failure(() => _adapter(fake).call('x', {})),
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 404)
            .having((e) => e.code, 'code', 'not_found'),
      );
    });

    test('a refusal keeps its members, details and challenge (cratestack#1243)', () async {
      final error = await failWith(
        TransportStatusError(
          statusCode: 401,
          headers: const {
            'content-type': 'application/cbor',
            'www-authenticate': 'Signature error="attestation_expired"',
          },
          body: Uint8List.fromList(
            cbor.cbor.encode({
              'code': 'unauthenticated',
              'message': 'attestation expired',
              'details': {'device': 'd-1'},
              'error': 'attestation_expired',
            }),
          ),
        ),
      );
      expect(
        error,
        isA<CratestackRpcException>()
            .having((e) => e.code, 'code', 'unauthenticated')
            .having((e) => e.details, 'details', {'device': 'd-1'})
            .having((e) => e.extra, 'extra', {'error': 'attestation_expired'})
            .having(
              (e) => e.challenges,
              'challenges',
              ['Signature error="attestation_expired"'],
            ),
      );
    });

    test('a step-up refusal is typed as stepUp (cratestack#1244)', () async {
      final error = await failWith(
        TransportStatusError(
          statusCode: 401,
          headers: const {'content-type': 'application/cbor'},
          body: Uint8List.fromList(
            cbor.cbor.encode({
              'code': 'unauthenticated',
              'message': 'step-up authentication required',
              'details': {
                'max_age': 300,
                'acr_values': ['aal2'],
                'unmet': ['max_age'],
              },
              'error': 'insufficient_user_authentication',
            }),
          ),
        ),
      );
      final stepUp = (error as CratestackRpcException).stepUp!;
      expect(stepUp.maxAge, 300);
      expect(stepUp.acrValues, ['aal2']);
      expect(stepUp.amr, isEmpty);
      expect(stepUp.unmet, ['max_age']);
    });

    test('any other error a decorator throws reaches the caller unchanged', () async {
      expect(await failWith(StateError('rejected')), isA<StateError>());
    });
  });

  group('providers', () {
    test('the transport and the base URI throw an instructive error until overridden', () {
      final container = ProviderContainer();
      addTearDown(container.dispose);
      expect(
        () => container.read(dartVerifyCiRpcAdapterProvider),
        throwsA(
          predicate(
            (Object e) => '$e'.contains('dartVerifyCiRpc'),
            'names the provider to override',
          ),
        ),
      );
    });

    test('overriding the transport and the base URI is all an app does', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      final container = ProviderContainer(
        overrides: [
          dartVerifyCiRpcTransportProvider.overrideWithValue(fake),
          dartVerifyCiRpcBaseUriProvider.overrideWithValue(
            Uri.parse('https://api.example.test/v1'),
          ),
        ],
      );
      addTearDown(container.dispose);
      await container.read(dartVerifyCiRpcClientProvider).adapter.call('x', {});
      expect(
        fake.requests.single.url.toString(),
        'https://api.example.test/v1/rpc/x',
      );
    });
  });
}
