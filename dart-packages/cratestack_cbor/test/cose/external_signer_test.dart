// The keystore callback signer (`ExternalSigner`, cratestack#1151): what each
// way a keystore call can end becomes, on whichever backend the test platform
// selects (`dart test` is native, `dart test -p chrome` is web): no
// `@TestOn`, on purpose, so both run it.
//
// None of it needs a real signature: a cancelled, slow or failing keystore
// ends the seal before anything is signed, and a signature that is not valid
// is exactly what the Rust-side check refuses. The byte-for-byte ESP256
// vectors are `vectors_body.dart`; the real P-256 round trip is
// `live_test.dart`.
import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';
import 'package:test/test.dart';

Uint8List unhex(String text) => Uint8List.fromList([
      for (var i = 0; i < text.length; i += 2)
        int.parse(text.substring(i, i + 2), radix: 16),
    ]);

Uint8List bytes(int length, [int start = 0]) =>
    Uint8List.fromList([for (var i = 0; i < length; i++) (start + i) & 0xff]);

/// `p256` of `keys.json`: the SEC1 point and its `kid`.
final p256Public = unhex(
  '0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb6'
  '7903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299',
);
final p256Kid = unhex('750ca40795ab23e5');

/// The same key as a `SubjectPublicKeyInfo`, as Android's `getEncoded()`.
final p256Spki = Uint8List.fromList([
  ...unhex('3059301306072a8648ce3d020106082a8648ce3d030107034200'),
  ...p256Public,
]);

/// `ed25519` of `keys.json`: any valid server key will do to open nothing.
final serverPublic = unhex(
  '03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8',
);

const shortTimeout = Duration(milliseconds: 150);
const longTimeout = Duration(seconds: 30);

CallBinding binding() => CallBinding(
      method: 'POST',
      route: 'procedure.echo',
      contractSha: bytes(32),
    );

/// A valid CBOR text string, so the payload is one an envelope may seal.
Uint8List cborText(String text) {
  final utf = utf8.encode(text);
  return Uint8List.fromList([0x60 + utf.length, ...utf]);
}

Future<ClientEnvelope> envelopeFor(
  SignCallback sign, {
  Uint8List? publicKey,
  Duration timeout = longTimeout,
}) =>
    ClientEnvelope.create(
      signer: ExternalSigner.esp256(
        publicKey: publicKey ?? p256Public,
        sign: sign,
        timeout: timeout,
      ),
      serverKeys: [CoseServerKey.ed25519(serverPublic)],
      audience: 'payments',
    );

Future<Uint8List> seal(ClientEnvelope envelope, [Uint8List? payload]) =>
    envelope.sealRequest(payload ?? cborText('hello'), binding());

bool hasBytes(Uint8List haystack, List<int> needle) {
  for (var i = 0; i + needle.length <= haystack.length; i++) {
    var hit = true;
    for (var j = 0; j < needle.length && hit; j++) {
      hit = haystack[i + j] == needle[j];
    }
    if (hit) return true;
  }
  return false;
}

void main() {
  group('how a keystore call can end', () {
    test('cancelled is CoseSignerCancelled, with no message', () async {
      final envelope =
          await envelopeFor((_) async => const SignOutcome.cancelled());
      await expectLater(
        seal(envelope),
        throwsA(isA<CoseSignerCancelled>().having((e) => e.message, 'm', '')),
      );
    });

    test('timedOut is CoseSignerTimedOut, with no message', () async {
      final envelope =
          await envelopeFor((_) async => const SignOutcome.timedOut());
      await expectLater(
        seal(envelope),
        throwsA(isA<CoseSignerTimedOut>().having((e) => e.message, 'm', '')),
      );
    });

    test('failed is CoseSignerFailed, with the callback\'s message', () async {
      final envelope = await envelopeFor(
        (_) async => const SignOutcome.failed('key invalidated'),
      );
      await expectLater(
        seal(envelope),
        throwsA(
          isA<CoseSignerFailed>()
              .having((e) => e.message, 'message', 'key invalidated'),
        ),
      );
    });

    test('a throwing callback is CoseSignerFailed, without its text', () async {
      for (final sign in <SignCallback>[
        (_) async => throw StateError('secret keystore detail'),
        // Thrown before a future exists.
        (_) => throw StateError('secret keystore detail'),
        (_) => Future<SignOutcome>.error(Exception('secret keystore detail')),
      ]) {
        final envelope = await envelopeFor(sign);
        await expectLater(
          seal(envelope),
          throwsA(
            isA<CoseSignerFailed>().having(
              (e) => e.message,
              'message',
              allOf(isNot(contains('secret')), contains('threw')),
            ),
          ),
        );
      }
    });

    test('a callback slower than the timeout is CoseSignerTimedOut', () async {
      final finished = Completer<void>();
      final envelope = await envelopeFor(
        (_) async {
          await Future<void>.delayed(const Duration(milliseconds: 600));
          finished.complete();
          return SignOutcome.signature(bytes(64));
        },
        timeout: shortTimeout,
      );
      await expectLater(seal(envelope), throwsA(isA<CoseSignerTimedOut>()));
      // The callback is not interrupted; its late answer is discarded, and
      // the envelope is still good for the next call.
      await finished.future;
      final cancelling =
          await envelopeFor((_) async => const SignOutcome.cancelled());
      await expectLater(seal(cancelling), throwsA(isA<CoseSignerCancelled>()));
    });

    test('a signature that is not valid is misuse, not a signer failure',
        () async {
      for (final signature in [
        Uint8List(0),
        bytes(3),
        bytes(64, 1), // the right size, signed by nobody
        Uint8List(64), // r = s = 0
      ]) {
        final envelope =
            await envelopeFor((_) async => SignOutcome.signature(signature));
        await expectLater(
          seal(envelope),
          throwsA(isA<CoseMisuse>()),
          reason: '${signature.length} bytes',
        );
      }
    });

    test('the callback is asked once per seal, for the whole to-be-signed',
        () async {
      final asked = <Uint8List>[];
      final envelope = await envelopeFor((tbs) async {
        asked.add(tbs);
        return const SignOutcome.cancelled();
      });
      final payload = cborText('whole-sig-structure');
      await expectLater(
        seal(envelope, payload),
        throwsA(isA<CoseSignerCancelled>()),
      );
      expect(asked, hasLength(1));
      // The Sig_structure carries the payload, not a digest of it.
      expect(hasBytes(asked.single, payload), isTrue);
      await expectLater(
        seal(envelope, payload),
        throwsA(isA<CoseSignerCancelled>()),
      );
      expect(asked, hasLength(2));
    });

    test('concurrent seals keep their own outcomes', () async {
      final cancel = cborText('cancel-me-please');
      final fail = cborText('fail-me-please');
      final envelope = await envelopeFor((tbs) async {
        if (hasBytes(tbs, cancel)) {
          await Future<void>.delayed(const Duration(milliseconds: 100));
          return const SignOutcome.cancelled();
        }
        return const SignOutcome.failed('the other one');
      });
      final results = await Future.wait([
        seal(envelope, cancel).then<Object>((_) => 'sealed',
            onError: (Object e) => e),
        seal(envelope, fail).then<Object>((_) => 'sealed',
            onError: (Object e) => e),
        seal(envelope, cancel).then<Object>((_) => 'sealed',
            onError: (Object e) => e),
      ]);
      expect(results[0], isA<CoseSignerCancelled>());
      expect(results[1], isA<CoseSignerFailed>());
      expect((results[1] as CoseSignerFailed).message, 'the other one');
      expect(results[2], isA<CoseSignerCancelled>());
    });
  });

  group('the public key', () {
    test('the kid is derived from it, whichever spelling it came in',
        () async {
      for (final key in [p256Public, p256Spki]) {
        final envelope = await envelopeFor(
          (_) async => const SignOutcome.cancelled(),
          publicKey: key,
        );
        expect(envelope.kid, p256Kid);
        expect(envelope.mode, CoseMode.sign1);
      }
    });

    test('a key that is not a P-256 point is misuse at creation', () async {
      final offCurve = Uint8List.fromList(p256Public)..[64] ^= 1;
      final wrongCurve = Uint8List.fromList(p256Spki)..[22] ^= 1; // prime256v1 -> 1.2.840.10045.3.1.6
      final trailing = Uint8List.fromList([...p256Spki, 0]);
      for (final key in [
        Uint8List(0),
        bytes(65),
        offCurve,
        wrongCurve,
        trailing,
      ]) {
        await expectLater(
          envelopeFor((_) async => const SignOutcome.cancelled(),
              publicKey: key),
          throwsA(isA<CoseMisuse>()),
          reason: '${key.length} bytes',
        );
      }
    });

    test('the key is copied: changing the argument changes nothing', () async {
      final key = Uint8List.fromList(p256Public);
      final signer = ExternalSigner.esp256(
        publicKey: key,
        sign: (_) async => const SignOutcome.cancelled(),
        timeout: longTimeout,
      );
      key[1] ^= 0xff;
      expect(signer.publicKey, p256Public);
    });
  });

  test('a timeout must be positive', () {
    for (final timeout in [Duration.zero, const Duration(seconds: -1)]) {
      expect(
        () => ExternalSigner.esp256(
          publicKey: p256Public,
          sign: (_) async => const SignOutcome.cancelled(),
          timeout: timeout,
        ),
        throwsArgumentError,
      );
    }
  });
}
