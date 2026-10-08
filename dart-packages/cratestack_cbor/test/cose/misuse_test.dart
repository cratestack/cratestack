// Misuse and the envelope's own facts, on whichever backend the test
// platform selects (`dart test` is native, `dart test -p chrome` is web):
// no `@TestOn`, on purpose, so both run it.
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';
import 'package:test/test.dart';

Uint8List bytes(int length, [int start = 0]) =>
    Uint8List.fromList([for (var i = 0; i < length; i++) (start + i) & 0xff]);

/// The public key of the seed `00 01 .. 1f` (from `keys.json`): a real
/// Ed25519 point, which an arbitrary 32 bytes are not.
final serverPublic = Uint8List.fromList([
  for (var i = 0; i < 64; i += 2)
    int.parse(
      '03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8'
          .substring(i, i + 2),
      radix: 16,
    ),
]);

CallBinding binding() => CallBinding(
      method: 'POST',
      route: 'procedure.echo',
      contractSha: bytes(32),
    );

Future<ClientEnvelope> ed25519({String audience = 'payments'}) =>
    ClientEnvelope.create(
      signer: Ed25519Signer.fromSeed(bytes(32)),
      serverKeys: [CoseServerKey.ed25519(serverPublic)],
      audience: audience,
    );

Matcher misuse() => isA<CoseMisuse>();

void main() {
  test('an empty audience is misuse', () {
    expect(ed25519(audience: ''), throwsA(misuse()));
  });

  test('a seed or a server key of the wrong length is misuse', () {
    expect(
      ClientEnvelope.create(
        signer: Ed25519Signer.fromSeed(bytes(31)),
        serverKeys: const [],
        audience: 'a',
      ),
      throwsA(misuse()),
    );
    expect(
      ClientEnvelope.create(
        signer: Ed25519Signer.fromSeed(bytes(32)),
        serverKeys: [CoseServerKey.ed25519(bytes(5))],
        audience: 'a',
      ),
      throwsA(misuse()),
    );
  });

  test('an HMAC secret under 32 bytes is misuse', () {
    expect(
      ClientEnvelope.create(
        signer: HmacSigner(CoseAlg.hmac256x64, bytes(8)),
        serverKeys: const [],
        audience: 'a',
      ),
      throwsA(misuse()),
    );
  });

  test('the Dart types refuse what cannot be right before any bridge call', () {
    expect(() => HmacSigner(CoseAlg.ed25519, bytes(32)), throwsArgumentError);
    expect(
      () => CoseServerKey.hmac(CoseAlg.esp256, bytes(32)),
      throwsArgumentError,
    );
    expect(
      () => CallBinding(method: 'GET', route: 'r', contractSha: bytes(31)),
      throwsArgumentError,
    );
  });

  test('the envelope describes itself', () async {
    final sign1 = await ed25519();
    expect(sign1.mode, CoseMode.sign1);
    expect(sign1.mediaType, 'application/cose; cose-type="cose-sign1"');
    expect(sign1.kid, hasLength(8));
    final secret = bytes(32, 0x40);
    final mac0 = await ClientEnvelope.create(
      signer: HmacSigner(CoseAlg.hmac256x64, secret),
      serverKeys: [CoseServerKey.hmac(CoseAlg.hmac256x64, secret)],
      audience: 'payments',
    );
    expect(mac0.mode, CoseMode.mac0);
    expect(mac0.mediaType, 'application/cose; cose-type="cose-mac0"');
    expect(mac0.kid, hasLength(8));
    expect(mac0.kid, isNot(sign1.kid));
  });

  test('the contract header value is the unbound 11 character selector',
      () async {
    await ed25519();
    expect(ClientEnvelope.contractHeaderValue(bytes(32)), 'AAECAwQFBgc');
    expect(
        () => ClientEnvelope.contractHeaderValue(bytes(5)), throwsA(misuse()));
  });

  test('real randomness: the same request never seals to the same bytes',
      () async {
    final envelope = await ed25519();
    final first = await envelope.sealRequest(bytes(4), binding());
    final second = await envelope.sealRequest(bytes(4), binding());
    expect(first, isNot(second));
    expect(first.length, second.length);
  });

  test('a body that is not a response to this request is rejected', () async {
    final envelope = await ed25519();
    final sealed = await envelope.sealRequest(bytes(4), binding());
    // A request is not a response, whatever key signed it.
    await expectLater(
      envelope.openResponse(
        sealed,
        binding: binding(),
        sealedRequest: sealed,
        status: 200,
      ),
      throwsA(isA<CoseRejected>()),
    );
    await expectLater(
      envelope.openResponse(
        bytes(3),
        binding: binding(),
        sealedRequest: sealed,
        status: 200,
      ),
      throwsA(isA<CoseRejected>()),
    );
  });

  test('every CoseException is handled by an exhaustive switch', () {
    String name(CoseException e) => switch (e) {
          CoseRejected() => 'rejected',
          CoseMisuse() => 'misuse',
          CoseSignerCancelled() => 'cancelled',
          CoseSignerTimedOut() => 'timed out',
          CoseSignerFailed() => 'failed',
        };
    expect(name(const CoseRejected()), 'rejected');
    expect(const CoseRejected().message, '');
    expect(const CoseMisuse('x').toString(), 'CoseMisuse: x');
  });
}
