// Fallback for a Dart compile target that is neither `dart.library.io` nor
// `dart.library.js_interop`; see `../unsupported_cbor_codec.dart`.
import 'dart:typed_data';

import 'cose_api.dart';
import 'types.dart';

Never _unsupported() => throw UnsupportedError(
      'cratestack_cbor: no COSE backend is available for this Dart compile '
      'target (neither dart.library.io nor dart.library.js_interop).',
    );

Future<ClientEnvelope> createEnvelope({
  required CoseSigner signer,
  required List<CoseServerKey> serverKeys,
  required String audience,
  SealPin? pin,
}) =>
    _unsupported();

String contractHeaderValue(Uint8List contractSha) => _unsupported();
