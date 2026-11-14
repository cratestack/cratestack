// A real round trip (cratestack#1026): the example server of
// `crates/cratestack-api` (`cose_roundtrip_server`) behind the COSE envelope
// layer, driven by `cratestack_cbor` with the REAL clock and REAL
// randomness (no pinned `iat` or `cti`), over HTTP with `package:http`.
// The server prints the op digests a generated client would bake in, then
// `listening <port>`.
//
// Three clients: Ed25519 and HMAC in memory, and ESP256 through an
// `ExternalSigner` over a software P-256 key (`software_keystore.dart`), the
// keystore callback signer of cratestack#1151.
//
// VM only: it spawns a process. Set CRATESTACK_COSE_SERVER to the built
// example (`cargo build -p cratestack-api --features cose --example
// cose_roundtrip_server`); `just cbor-verify-package` does. Without it the
// tests skip, unless CRATESTACK_COSE_REQUIRE_LIVE=1, which fails them.
@TestOn('vm')
@Tags(['live'])
library;

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:cratestack_cbor/cose.dart';
import 'package:http/http.dart' as http;
import 'package:test/test.dart';

import 'live_support.dart';

void main() {
  final executable = Platform.environment['CRATESTACK_COSE_SERVER'];
  final require = Platform.environment['CRATESTACK_COSE_REQUIRE_LIVE'] == '1';
  if (executable == null || executable.isEmpty) {
    test('the live COSE round trip', () {
      if (require) {
        fail('CRATESTACK_COSE_SERVER is not set, and live tests are required');
      }
    }, skip: require ? false : 'CRATESTACK_COSE_SERVER is not set');
    return;
  }

  for (final (mode, kind) in [
    ('sign1', 'ed25519'),
    ('sign1', 'esp256'),
    ('mac0', 'hmac'),
  ]) {
    group('$mode/$kind', () {
      late Server server;
      late ClientEnvelope envelope;
      late CratestackCborCodec codec;
      late http.Client client;

      setUpAll(() async {
        server = await Server.start(executable, mode);
        codec = await createCborCodec();
        final creds = credentials(mode, kind);
        envelope = await ClientEnvelope.create(
          signer: creds.signer,
          serverKeys: [creds.server],
          audience: audience,
        );
        client = http.Client();
      });

      tearDownAll(() {
        client.close();
        server.stop();
      });

      CallBinding bindingFor(String route, {String? idempotencyKey}) =>
          CallBinding(
            method: 'POST',
            route: route,
            contractSha: server.contracts[route]!,
            idempotencyKey: idempotencyKey,
          );

      Uint8List payload(String message) => codec.encodeJson(
            jsonEncode({
              'args': {'message': message},
            }),
          );

      Uri url(String route) => Uri.parse(
            route.startsWith('/')
                ? 'http://127.0.0.1:${server.port}$route'
                : 'http://127.0.0.1:${server.port}/rpc/$route',
          );

      Future<http.Response> post(
        String route,
        Uint8List body,
        CallBinding binding, {
        Map<String, String> extra = const {},
      }) =>
          client.post(
            url(route),
            headers: {
              'Content-Type': envelope.mediaType,
              'Accept': envelope.mediaType,
              'Cratestack-Contract': ClientEnvelope.contractHeaderValue(
                binding.contractSha,
              ),
              ...extra,
            },
            body: body,
          );

      for (final route in ['procedure.echo', r'/$procs/echo']) {
        final transport = route.startsWith('/') ? 'REST' : 'RPC';

        test('$transport: seal, send, open', () async {
          final binding = bindingFor(route);
          final sealed = await envelope.sealRequest(payload('hello'), binding);
          final response = await post(route, sealed, binding);
          expect(response.statusCode, 200, reason: response.body);
          final opened = await envelope.openResponse(
            response.bodyBytes,
            binding: binding,
            sealedRequest: sealed,
            status: response.statusCode,
          );
          // The server signs as its mode says, whatever the client signed with.
          expect(opened.alg,
              mode == 'sign1' ? CoseAlg.ed25519 : CoseAlg.hmac256x64);
          expect(opened.kid, hasLength(8));
          final reply = jsonDecode(codec.decodeJson(opened.payload))
              as Map<String, dynamic>;
          expect(reply['message'], 'hello');
          expect(reply['signer'], 'Some(${credentials(mode, kind).alg})');
        });

        test('$transport: a replay is refused with 401', () async {
          final binding = bindingFor(route);
          final sealed = await envelope.sealRequest(payload('once'), binding);
          expect((await post(route, sealed, binding)).statusCode, 200);
          expect((await post(route, sealed, binding)).statusCode, 401);
        });

        test('$transport: a flipped byte is refused with 401', () async {
          final binding = bindingFor(route);
          final sealed = await envelope.sealRequest(payload('flip'), binding);
          for (final at in [0, sealed.length ~/ 2, sealed.length - 1]) {
            final tampered = Uint8List.fromList(sealed)..[at] ^= 1;
            expect(
              (await post(route, tampered, binding)).statusCode,
              401,
              reason: 'byte $at',
            );
          }
        });
      }

      if (kind == 'esp256') {
        test('the kid is the thumbprint of the keystore key', () {
          expect(envelope.kid, unhex(p256Kid));
        });
      }

      test('REST: the Idempotency-Key is bound, sent and echoed in the binding',
          () async {
        const route = r'/$procs/echo';
        final binding = bindingFor(route, idempotencyKey: 'idem-live-1');
        final sealed = await envelope.sealRequest(payload('keyed'), binding);
        final response = await post(
          route,
          sealed,
          binding,
          extra: {'Idempotency-Key': 'idem-live-1'},
        );
        expect(response.statusCode, 200, reason: response.body);
        final opened = await envelope.openResponse(
          response.bodyBytes,
          binding: binding,
          sealedRequest: sealed,
          status: 200,
        );
        expect(
          jsonDecode(codec.decodeJson(opened.payload))['message'],
          'keyed',
        );
        // The response is bound to the key too: the same bytes do not open
        // under a binding that names another.
        await expectLater(
          envelope.openResponse(
            response.bodyBytes,
            binding: bindingFor(route, idempotencyKey: 'idem-live-2'),
            sealedRequest: sealed,
            status: 200,
          ),
          throwsA(isA<CoseRejected>()),
        );
        // A header that differs from the bound key is refused.
        final other = await envelope.sealRequest(payload('keyed'), binding);
        final mismatch = await post(
          route,
          other,
          binding,
          extra: {'Idempotency-Key': 'idem-live-OTHER'},
        );
        expect(mismatch.statusCode, 401);
      });
    });
  }
}
