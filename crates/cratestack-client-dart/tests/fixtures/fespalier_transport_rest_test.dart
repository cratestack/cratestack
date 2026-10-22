// cratestack#1204, `--preset fespalier` (REST): the generated
// `CratestackFespalierTransport` over fespalier_transport's `FakeTransport`.
//
// Proves, against the real fespalier_transport interface:
//  * every REST operation reaches the transport as the same request the Dio
//    adapters used to send (`wire-golden/rest-*.json`, the golden files of
//    `wire_parity_rest_test.dart`);
//  * the CBOR bytes a decorator sees are the exact bytes the inner transport
//    receives (a signer layered above sees what goes on the wire);
//  * an abort through `TransportZone` (`ref.cancellable`'s mechanism) fires,
//    and a call option's `abort` aborts too (either one aborts);
//  * every `TransportFailure` maps to the matching `CratestackTransportException`,
//    a `TransportStatusError` becomes the typed REST error, and any other
//    error a decorator throws passes through unchanged;
//  * the generated providers: the transport and the base URI throw an
//    instructive error until overridden, and the client sends through them.
//
// `just verify-dart-fespalier` copies this file, `wire_parity_rest_ops.dart`
// and `wire-golden/` into the fespalier-preset package generated from
// `ci_rest.cstack` (library name `dart_verify_ci_rest`).

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:fespalier_transport/testing.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'wire_parity_rest_ops.dart';

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

CratestackHttpAdapter _adapter(
  HttpTransport transport, {
  CratestackWireFormat format = CratestackWireFormat.cbor,
  bool useRustTransport = false,
}) => CratestackHttpAdapter(
  CratestackFespalierTransport(transport),
  baseUri: Uri.parse('http://127.0.0.1:9'),
  format: format,
  useRustTransport: useRustTransport,
);

const _get = CratestackRequest(method: 'GET', path: '/api/posts');

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

void main() {
  group('wire parity through fespalier_transport', () {
    for (final format in CratestackWireFormat.values) {
      test('${format.name}: every REST operation sends the golden request', () async {
        final golden = jsonDecode(
          File('test/wire-golden/rest-${format.name}.json').readAsStringSync(),
        );
        final fake = FakeTransport((_, __) => fakeResponse(204));
        await runRestOps(
          DartVerifyCiRestCratestackClient(
            _adapter(fake, format: format),
            basePath: '/api',
          ),
        );
        expect(fake.requests.map(_normalize).toList(), equals(golden));
      });
    }

    test('a decorator sees the exact CBOR bytes the inner transport receives', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      final signer = _Signer(fake);
      await _adapter(signer).execute(
        const CratestackRequest(
          method: 'POST',
          path: '/api/posts',
          body: {'title': 'hi', 'n': 1},
        ),
      );
      final sent = fake.requests.single;
      expect(sent.lowerHeaders['content-type'], 'application/cbor');
      expect(signer.seen, isNotNull);
      expect(sent.body, orderedEquals(signer.seen!));
      expect(cbor.cbor.decode(sent.body), {'title': 'hi', 'n': 1});
    });

    test('extra travels as attributes, never as a header', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      await _adapter(fake, useRustTransport: true).execute(_get);
      final request = fake.requests.single.request;
      expect(request.attributes[cratestackUseRustTransportExtraKey], isTrue);
      expect(
        request.headers.keys,
        isNot(contains(cratestackUseRustTransportExtraKey)),
      );
    });

    test('the answer is decoded by its own Content-Type', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, 'application/cbor', cbor.cbor.encode({'n': 2})),
      );
      expect(await _adapter(fake).execute(_get), {'n': 2});
    });
  });

  group('abort', () {
    test('the abort trigger of the current TransportZone aborts the send', () async {
      final fake = FakeTransport((_, __) => Completer<TransportResponse>().future);
      final trigger = Completer<void>();
      final call = TransportZone.run(trigger.future, () => _adapter(fake).execute(_get));
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
        () => _adapter(fake).execute(
          _get,
          options: CratestackCallOptions(abort: option.future),
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
        () => _adapter(fake).execute(
          _get,
          options: CratestackCallOptions(abort: option.future),
        ),
      );
      final outcome = expectLater(call, throwsA(isA<CratestackTransportAborted>()));
      await pumpEventQueue();
      zone.complete();
      await outcome;
      expect(fake.abortCount, 1);
      expect(option.isCompleted, isFalse);
    });

    test('a call outside any zone and without an abort is not abortable', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      await _adapter(fake).execute(_get);
      expect(fake.requests.single.abortable, isFalse);
    });
  });

  group('failures', () {
    Future<Object> failWith(Object thrown) => _failure(
      () => _adapter(FakeTransport((_, __) => throw thrown)).execute(_get),
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

    test('a failure while the body streams maps too', () async {
      final fake = FakeTransport(
        (_, __) => TransportResponse(
          statusCode: 200,
          headers: const {'content-type': 'application/cbor'},
          body: Stream<List<int>>.error(const TransportBroken('reset')),
        ),
      );
      expect(
        await _failure(() => _adapter(fake).execute(_get)),
        isA<CratestackTransportBroken>(),
      );
    });

    test('a TransportStatusError is treated as the answer it carries', () async {
      final error = await failWith(
        TransportStatusError(
          statusCode: 409,
          headers: const {'content-type': 'application/json'},
          body: Uint8List.fromList(utf8.encode('{"code":"conflict"}')),
        ),
      );
      expect(
        error,
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 409)
            .having((e) => e.body, 'body', {'code': 'conflict'}),
      );
    });

    test('an error answer is a CratestackRestException, not a transport error', () async {
      final fake = FakeTransport(
        (_, __) => _answer(422, 'application/cbor', cbor.cbor.encode({'message': 'bad'})),
      );
      expect(
        await _failure(() => _adapter(fake).execute(_get)),
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 422)
            .having((e) => e.body, 'body', {'message': 'bad'}),
      );
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
        () => container.read(dartVerifyCiRestAdapterProvider),
        throwsA(
          predicate(
            (Object e) => '$e'.contains('dartVerifyCiRest'),
            'names the provider to override',
          ),
        ),
      );
    });

    test('overriding the transport and the base URI is all an app does', () async {
      final fake = FakeTransport((_, __) => fakeResponse(204));
      final container = ProviderContainer(
        overrides: [
          dartVerifyCiRestTransportProvider.overrideWithValue(fake),
          dartVerifyCiRestBaseUriProvider.overrideWithValue(
            Uri.parse('https://api.example.test/v1'),
          ),
        ],
      );
      addTearDown(container.dispose);
      await container
          .read(dartVerifyCiRestClientProvider)
          .execute('GET', '/posts');
      expect(
        fake.requests.single.url.toString(),
        'https://api.example.test/v1/api/posts',
      );
    });
  });
}
