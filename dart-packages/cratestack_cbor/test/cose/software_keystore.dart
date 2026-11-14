// A software P-256 key standing in for a platform keystore, for the live
// round trip (`live_test.dart`) only. This is the one place Dart signs
// anything: `pointycastle` is a dev dependency of this package and never a
// runtime one, and the library itself contains no crypto (every signature
// the envelope sends is checked in Rust before it leaves).
import 'dart:typed_data';

import 'package:pointycastle/export.dart';

import 'esp256_oracle.dart';

/// A P-256 key that signs like `SHA256withECDSA` and answers in DER.
final class SoftwareKeystore {
  /// The key with private scalar [scalarHex] (64 hex characters).
  SoftwareKeystore(String scalarHex)
      : _domain = ECDomainParameters('prime256v1') {
    _private = ECPrivateKey(BigInt.parse(scalarHex, radix: 16), _domain);
  }

  final ECDomainParameters _domain;
  late final ECPrivateKey _private;

  /// The uncompressed SEC1 public key, 65 bytes.
  Uint8List get publicKey =>
      (_domain.G * _private.d!)!.getEncoded(false);

  /// ECDSA over SHA-256 of [message], DER encoded. The message is hashed
  /// here, as a keystore does: pass the bytes as given, never a digest.
  Uint8List signDer(Uint8List message) {
    final signer = ECDSASigner(SHA256Digest(), HMac(SHA256Digest(), 64))
      ..init(true, PrivateKeyParameter<ECPrivateKey>(_private));
    final signature = signer.generateSignature(message) as ECSignature;
    return spell(
      Uint8List.fromList([...bytes32(signature.r), ...bytes32(signature.s)]),
      Spelling.der,
    );
  }
}
