/// COSE signed transport for CrateStack Dart/Flutter clients (ADR 0006,
/// cratestack#1026): seal a request and open a response with
/// `cratestack-cose`, the one Rust implementation, reached through the same
/// backends as the CBOR codec: `flutter_rust_bridge` natively and the
/// `cratestack-cbor-wasm` build on the web. Nothing here implements COSE or
/// any crypto in Dart.
///
/// Import it next to the codec; a codec-only app imports nothing new:
///
/// ```dart
/// import 'package:cratestack_cbor/cose.dart';
///
/// final envelope = await ClientEnvelope.create(
///   signer: Ed25519Signer.fromSeed(seed),
///   serverKeys: [CoseServerKey.ed25519(serverPublicKey)],
///   audience: 'payments',
/// );
/// ```
///
/// This client is Required-only, like the Rust client: it seals every
/// request and opens only responses to a sealed request.
library;

export 'src/cose/cose_api.dart' show ClientEnvelope;
export 'src/cose/errors.dart';
export 'src/cose/types.dart';
