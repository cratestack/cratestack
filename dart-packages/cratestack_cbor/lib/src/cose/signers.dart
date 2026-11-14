import 'dart:typed_data';

import 'sign_outcome.dart';
import 'types.dart';

/// Who signs a request. Sealed: [HmacSigner] and [Ed25519Signer] hold their
/// key in memory (service credentials and tests); [ExternalSigner] never has
/// the key, which stays in the Android Keystore, the Secure Enclave or
/// WebCrypto, and only asks it to sign.
sealed class CoseSigner {
  const CoseSigner();
}

/// Signs a COSE_Mac0 with a shared secret of at least 32 random bytes.
///
/// The secret lives in this process's memory: for service credentials and
/// tests, never for an end user's device. The object keeps its copy
/// ([secret]) for as long as it is reachable, and Dart cannot wipe memory,
/// so drop the signer once the [ClientEnvelope] is built. The Rust side wipes
/// its own copy when the envelope is dropped. [secret] is public only because
/// the backends, in other libraries, read it.
///
/// Throws [ArgumentError] if [alg] is not an HMAC algorithm.
final class HmacSigner extends CoseSigner {
  /// [alg] must be [CoseAlg.hmac256x64] or [CoseAlg.hmac256x256].
  HmacSigner(this.alg, Uint8List secret) : secret = Uint8List.fromList(secret) {
    if (!alg.isHmac) {
      throw ArgumentError.value(alg, 'alg', 'must be an HMAC algorithm');
    }
  }

  /// The HMAC algorithm.
  final CoseAlg alg;

  /// The shared secret (a copy).
  final Uint8List secret;
}

/// Signs a COSE_Sign1 with the Ed25519 key of a 32-byte seed.
///
/// **In memory only: never a device key.** The seed is held in this
/// process's memory, so use it for tests and service credentials. A key
/// that must survive on a phone belongs in the platform keystore
/// ([ExternalSigner]). The object
/// keeps its copy ([seed]) for as long as it is reachable (Dart cannot wipe
/// memory), so drop the signer once the [ClientEnvelope] is built; [seed] is
/// public only because the backends, in other libraries, read it. A seed of
/// the wrong length is [CoseMisuse] when the envelope is created.
final class Ed25519Signer extends CoseSigner {
  /// The key derived from [seed] (32 bytes).
  Ed25519Signer.fromSeed(Uint8List seed) : seed = Uint8List.fromList(seed);

  /// The seed (a copy).
  final Uint8List seed;
}

/// Signs a COSE_Sign1 (ESP256) with a P-256 key that never enters this
/// process: one in the Android Keystore, the iOS Secure Enclave, WebCrypto, or
/// any other store that can sign but not export (cratestack#1151).
///
/// Only the key's **public** half is given here. For each request the
/// envelope hands [sign] the complete to-be-signed bytes and expects an ECDSA
/// signature over SHA-256 of exactly those bytes; everything else (the CBOR,
/// the binding, DER to raw `r || s`, low-`s`, and checking the signature
/// verifies under [publicKey] before anything is sent) is `cratestack-cose`'s,
/// so [sign] does none of it. The `kid` is derived from [publicKey]
/// (`ClientEnvelope.kid`), never supplied.
///
/// How to sign, **passing the bytes as given and never a digest** (the
/// platform hashes for you):
///
/// - Android: `Signature.getInstance("SHA256withECDSA")`, `update(tbs)`,
///   `sign()` (DER).
/// - iOS: `SecKeyCreateSignature(key, .ecdsaSignatureMessageX962SHA256, tbs)`
///   (DER).
/// - Web: `crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, key, tbs)`
///   (raw `r || s`).
///
/// [publicKey] is a SEC1 point (uncompressed, 65 bytes `0x04 || x || y`, or
/// compressed, 33 bytes) or a P-256 `SubjectPublicKeyInfo`, which is what
/// Android's `PublicKey.getEncoded()` returns. This is asymmetric with the
/// server keys ([CoseServerKey.p256Sec1]), which are SEC1 only. A key that is neither, or not on the curve, is [CoseMisuse] when
/// the envelope is created.
///
/// [sign] returns a [SignOutcome] and must not throw: report a dismissed
/// prompt as [SignOutcome.cancelled] and a keystore failure as
/// [SignOutcome.failed]. A throw is still caught and becomes
/// [CoseSignerFailed] (without the exception's text). A signature that is not
/// a valid ECDSA signature by [publicKey] is [CoseMisuse], not a signer
/// failure.
///
/// [timeout] bounds each call; there is no default, because a keystore prompt
/// can wait on a person for as long as they like and only the app knows how
/// long a request may. Exceeding it is [CoseSignerTimedOut]. **It does not
/// cancel the platform prompt**: the callback keeps running and its late
/// answer is discarded.
final class ExternalSigner extends CoseSigner {
  /// An ESP256 signer for the key whose public half is [publicKey].
  ///
  /// Throws [ArgumentError] if [timeout] is not positive.
  ExternalSigner.esp256({
    required Uint8List publicKey,
    required this.sign,
    required this.timeout,
  }) : publicKey = Uint8List.fromList(publicKey) {
    if (timeout <= Duration.zero) {
      throw ArgumentError.value(timeout, 'timeout', 'must be positive');
    }
  }

  /// The public key (a copy): a SEC1 point (uncompressed or compressed) or a
  /// `SubjectPublicKeyInfo`.
  final Uint8List publicKey;

  /// Signs the to-be-signed bytes it is given with the keystore.
  final SignCallback sign;

  /// How long one [sign] call may take.
  final Duration timeout;
}
