import 'dart:typed_data';

/// The four algorithms of ADR 0006 §2.
enum CoseAlg {
  /// Ed25519 (`-19`), COSE_Sign1.
  ed25519,

  /// ESP256 (`-9`), COSE_Sign1, low-`s` only.
  esp256,

  /// HMAC 256/64 (`4`), COSE_Mac0.
  hmac256x64,

  /// HMAC 256/256 (`5`), COSE_Mac0.
  hmac256x256;

  /// Whether this is one of the two COSE_Mac0 algorithms.
  bool get isHmac => this == hmac256x64 || this == hmac256x256;
}

/// The message structure an envelope emits and accepts.
enum CoseMode {
  /// COSE_Sign1 (tag 18): asymmetric, the verifier cannot forge.
  sign1,

  /// COSE_Mac0 (tag 17): symmetric, for service to service inside one trust
  /// domain.
  mac0,
}

/// Who signs a request. Sealed: the signers of this release hold their key
/// in memory; a key in the Android Keystore or the Secure Enclave comes with
/// the callback signer of a later release.
sealed class CoseSigner {
  const CoseSigner();
}

/// Signs a COSE_Mac0 with a shared secret of at least 32 random bytes.
///
/// The secret lives in this process's memory: for service credentials and
/// tests, never for an end user's device.
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
/// that must survive on a phone belongs in the platform keystore.
final class Ed25519Signer extends CoseSigner {
  /// The key derived from [seed] (32 bytes).
  Ed25519Signer.fromSeed(Uint8List seed) : seed = Uint8List.fromList(seed);

  /// The seed (a copy).
  final Uint8List seed;
}

/// A key the server's responses verify with, pinned at enrolment.
final class CoseServerKey {
  const CoseServerKey._(this.alg, this.bytes);

  /// An Ed25519 public key (32 bytes).
  factory CoseServerKey.ed25519(Uint8List publicKey) =>
      CoseServerKey._(CoseAlg.ed25519, Uint8List.fromList(publicKey));

  /// A P-256 public key, SEC1 encoded (compressed or uncompressed), for ESP256.
  factory CoseServerKey.p256Sec1(Uint8List sec1) =>
      CoseServerKey._(CoseAlg.esp256, Uint8List.fromList(sec1));

  /// The shared secret of a COSE_Mac0 server, for [alg] (an HMAC algorithm).
  factory CoseServerKey.hmac(CoseAlg alg, Uint8List secret) {
    if (!alg.isHmac) {
      throw ArgumentError.value(alg, 'alg', 'must be an HMAC algorithm');
    }
    return CoseServerKey._(alg, Uint8List.fromList(secret));
  }

  /// The one algorithm this key verifies.
  final CoseAlg alg;

  /// The key material: a public key, a SEC1 point or a secret.
  final Uint8List bytes;
}

/// The inputs a call's signature is bound to (ADR 0006 §4). The audience is
/// the envelope's, not the call's.
final class CallBinding {
  /// [contractSha] is the op's contract digest, 32 bytes.
  CallBinding({
    required this.method,
    required this.route,
    this.pathParams = const [],
    this.query,
    required Uint8List contractSha,
    this.idempotencyKey,
    this.ifMatch,
  }) : contractSha = Uint8List.fromList(contractSha) {
    if (contractSha.length != 32) {
      throw ArgumentError.value(
        contractSha.length,
        'contractSha',
        'the op contract digest is 32 bytes',
      );
    }
  }

  /// HTTP method, e.g. `POST`.
  final String method;

  /// The RPC `op_id`, or the REST route template.
  final String route;

  /// REST path parameter values in template order; empty for RPC.
  final List<String> pathParams;

  /// The query string in any spelling; it is canonicalised before it binds.
  final String? query;

  /// The op contract digest (32 bytes).
  final Uint8List contractSha;

  /// The `Idempotency-Key` the request will carry, bound exactly as sent.
  final String? idempotencyKey;

  /// The `If-Match` the request will carry, bound exactly as sent.
  final String? ifMatch;
}

/// A response that verified.
final class Opened {
  /// Builds a result.
  const Opened({
    required this.payload,
    required this.kid,
    required this.alg,
    required this.thumbprint,
  });

  /// The payload, exactly as signed (CBOR).
  final Uint8List payload;

  /// The signer's 8-byte `kid`.
  final Uint8List kid;

  /// The algorithm it verified with.
  final CoseAlg alg;

  /// The RFC 9679 thumbprint of the key that verified (32 bytes).
  final Uint8List thumbprint;
}
