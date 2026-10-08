// The shared COSE vectors (`crates/cratestack-cose/tests/vectors/*.json`,
// read in place, never copied) driven through the PUBLIC Dart API. Both
// backends run this one body: `vectors_vm_test.dart` loads the JSON with
// `dart:io`, `vectors_web_test.dart` through `spawnHybridUri`.
//
// Requests: every case this client can sign must seal byte for byte at the
// vector's fixed `iat` and `cti`. The four ESP256 requests cannot be
// signed from here (this release has no ESP256 signer: that is the
// keystore callback signer's job); `cratestack-client-flutter` and
// `cratestack-cbor-wasm` check those bytes in Rust. Responses: the 12 that
// answer a signed request open; the 4 that answer an unsigned one are not
// this client's (it is Required-only, like the Rust client).
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';
import 'package:cratestack_cbor/cose_testing.dart';
import 'package:test/test.dart';

typedef Json = Map<String, dynamic>;

Uint8List unhex(String text) => Uint8List.fromList([
      for (var i = 0; i < text.length; i += 2)
        int.parse(text.substring(i, i + 2), radix: 16),
    ]);

CallBinding binding(Json json) => CallBinding(
      method: json['method'] as String,
      route: json['route'] as String,
      pathParams: (json['path_params'] as List).cast<String>(),
      query: json['query'] as String?,
      contractSha: unhex(json['contract_sha'] as String),
      idempotencyKey:
          (json['bound_headers'] as Json)['idempotency_key'] as String?,
      ifMatch: (json['bound_headers'] as Json)['if_match'] as String?,
    );

/// The signer a `keys.json` entry names, or `null` for ESP256 (see above).
CoseSigner? signerFor(String name, Json key) => switch (name) {
      'ed25519' => Ed25519Signer.fromSeed(unhex(key['seed'] as String)),
      'p256' => null,
      _ => HmacSigner(
          key['alg'] == 4 ? CoseAlg.hmac256x64 : CoseAlg.hmac256x256,
          unhex(key['secret'] as String),
        ),
    };

/// The server key a response signed by `name` verifies with.
CoseServerKey serverKeyFor(String name, Json key) => switch (name) {
      'ed25519' => CoseServerKey.ed25519(unhex(key['public'] as String)),
      'p256' => CoseServerKey.p256Sec1(
          unhex(key['public_sec1_uncompressed'] as String),
        ),
      _ => CoseServerKey.hmac(
          key['alg'] == 4 ? CoseAlg.hmac256x64 : CoseAlg.hmac256x256,
          unhex(key['secret'] as String),
        ),
    };

/// Registers the vector tests against [load], which returns
/// `(keys.json, unary.json)`.
void defineVectorTests(Future<(Json, Json)> Function() load) {
  late Json keys;
  late Json unary;
  late List<Json> cases;

  setUpAll(() async {
    (keys, unary) = await load();
    cases = (unary['cases'] as List).cast<Json>();
  });

  test('every request this client can sign seals byte for byte', () async {
    var sealed = 0;
    var skippedEsp256 = 0;
    for (final c in cases.where((c) => c['direction'] == 'request')) {
      final name = c['name'] as String;
      final keyName = c['key'] as String;
      final key = keys[keyName] as Json;
      final signer = signerFor(keyName, key);
      if (signer == null) {
        skippedEsp256++;
        continue;
      }
      final envelope = await ClientEnvelopeForVectors.forVectors(
        signer: signer,
        serverKeys: [serverKeyFor(keyName, key)],
        audience: 'payments',
        iat: c['iat'] as int,
        cti: unhex(c['cti'] as String),
      );
      final got = await envelope.sealRequest(
        unhex(c['payload'] as String),
        binding(c['binding'] as Json),
      );
      expect(got, unhex(c['cose'] as String), reason: name);
      expect(envelope.kid, unhex(key['kid'] as String), reason: name);
      sealed++;
    }
    // 13 of the 17 request vectors (16 plus the empty-query twin).
    expect((sealed, skippedEsp256), (13, 4));
  });

  test('every response to a signed request opens; unsigned ones are skipped',
      () async {
    var opened = 0;
    var skipped = 0;
    for (final c in cases.where((c) => c['direction'] == 'response')) {
      final name = c['name'] as String;
      final of = c['request_digest_of'] as String?;
      if (of == null) {
        skipped++;
        continue;
      }
      final request = cases.firstWhere((other) => other['name'] == of);
      final keyName = c['key'] as String;
      final key = keys[keyName] as Json;
      // Any signer of the response's mode will do to open it.
      final envelope = await ClientEnvelope.create(
        signer: signerFor(keyName, key) ??
            signerFor('ed25519', keys['ed25519'] as Json)!,
        serverKeys: [serverKeyFor(keyName, key)],
        audience: 'payments',
      );
      final result = await envelope.openResponse(
        unhex(c['cose'] as String),
        binding: binding(c['binding'] as Json),
        sealedRequest: unhex(request['cose'] as String),
        status: (c['binding'] as Json)['status'] as int,
      );
      expect(result.payload, unhex(c['payload'] as String), reason: name);
      expect(result.kid, unhex(key['kid'] as String), reason: name);
      expect(result.thumbprint, unhex(key['thumbprint'] as String));
      expect(result.alg.index, isNotNull);
      opened++;
    }
    expect((opened, skipped), (12, 4));
  });

  test('a tampered response is rejected, and says nothing else', () async {
    for (final c in cases.where(
      (c) => c['direction'] == 'response' && c['request_digest_of'] != null,
    )) {
      final name = c['name'] as String;
      final request = cases.firstWhere(
        (other) => other['name'] == c['request_digest_of'],
      );
      final keyName = c['key'] as String;
      final key = keys[keyName] as Json;
      final envelope = await ClientEnvelope.create(
        signer: signerFor(keyName, key) ??
            signerFor('ed25519', keys['ed25519'] as Json)!,
        serverKeys: [serverKeyFor(keyName, key)],
        audience: 'payments',
      );
      final body = unhex(c['cose'] as String);
      final sealedRequest = unhex(request['cose'] as String);
      final call = binding(c['binding'] as Json);
      final status = (c['binding'] as Json)['status'] as int;
      Future<void> rejected(Uint8List body, Uint8List sealed, int status,
          [CallBinding? other]) async {
        await expectLater(
          envelope.openResponse(body,
              binding: other ?? call, sealedRequest: sealed, status: status),
          throwsA(
            isA<CoseRejected>().having((e) => e.message, 'message', ''),
          ),
          reason: name,
        );
      }

      for (final at in [0, body.length ~/ 2, body.length - 1]) {
        await rejected(
            Uint8List.fromList(body)..[at] ^= 1, sealedRequest, status);
      }
      await rejected(body, Uint8List.fromList(sealedRequest)..[0] ^= 1, status);
      await rejected(body, sealedRequest, status ^ 1);
      await rejected(
        body,
        sealedRequest,
        status,
        CallBinding(
          method: call.method,
          route: '${call.route}x',
          pathParams: call.pathParams,
          query: call.query,
          contractSha: call.contractSha,
          idempotencyKey: call.idempotencyKey,
          ifMatch: call.ifMatch,
        ),
      );
    }
  });
}
