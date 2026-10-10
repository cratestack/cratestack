/// Test support for `package:cratestack_cbor/cose.dart`: an envelope whose
/// `iat` and `cti` are pinned, so the shared vectors
/// (`crates/cratestack-cose/tests/vectors`) reproduce byte for byte.
///
/// **Never in production code**: a pinned `cti` is a replayed request.
library;

import 'dart:typed_data';

import 'cose.dart';
import 'src/cose/cose_api.dart' show SealPin, createPinnedEnvelope;

/// `ClientEnvelope.forVectors`, as a static on an extension (Dart cannot add
/// a static to a class from another library).
extension ClientEnvelopeForVectors on ClientEnvelope {
  /// An envelope like [ClientEnvelope.create] whose `iat` is [iat] (Unix
  /// seconds) and whose `cti` is [cti].
  static Future<ClientEnvelope> forVectors({
    required CoseSigner signer,
    required List<CoseServerKey> serverKeys,
    required String audience,
    required int iat,
    required Uint8List cti,
  }) =>
      createPinnedEnvelope(
        signer: signer,
        serverKeys: serverKeys,
        audience: audience,
        pin: SealPin(iat: iat, cti: cti),
      );
}
