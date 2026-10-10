import 'dart:typed_data';

import 'errors.dart';
import 'types.dart';
import 'unsupported_cose.dart'
    if (dart.library.io) 'native/native_cose.dart'
    if (dart.library.js_interop) 'web/web_cose.dart' as backend;

/// Seals requests and opens responses for one service (ADR 0006).
///
/// All crypto, the canonical query and the binding are `cratestack-cose`'s
/// one Rust implementation, reached over `flutter_rust_bridge` on native
/// platforms and a wasm-bindgen build on the web. There is no Dart
/// reimplementation.
///
/// ```dart
/// final envelope = await ClientEnvelope.create(
///   signer: HmacSigner(CoseAlg.hmac256x64, secret),
///   serverKeys: [CoseServerKey.hmac(CoseAlg.hmac256x64, secret)],
///   audience: 'payments',
/// );
/// final sealed = await envelope.sealRequest(payload, binding);
/// // POST `sealed` with Content-Type: envelope.mediaType and
/// // Cratestack-Contract: ClientEnvelope.contractHeaderValue(sha) ...
/// final opened = await envelope.openResponse(
///   responseBody,
///   binding: binding,
///   sealedRequest: sealed,
///   status: 200,
/// );
/// ```
///
/// Every failure is a [CoseException].
abstract interface class ClientEnvelope {
  /// An envelope for [audience] (the configured name of the service, never
  /// its host) that signs with [signer] and opens responses with
  /// [serverKeys], and starts the backend if nothing has yet.
  ///
  /// Throws [CoseMisuse] for a key of the wrong shape or an empty audience.
  static Future<ClientEnvelope> create({
    required CoseSigner signer,
    required List<CoseServerKey> serverKeys,
    required String audience,
  }) =>
      backend.createEnvelope(
        signer: signer,
        serverKeys: serverKeys,
        audience: audience,
      );

  /// The `Cratestack-Contract` header value for an op contract digest (32
  /// bytes): 11 characters of unpadded base64url. The header is not bound;
  /// it tells a server that accepts several digests which one the request
  /// was sealed under.
  ///
  /// Needs the backend started, so call it after [create] (or after
  /// `createCborCodec`).
  static String contractHeaderValue(Uint8List contractSha) =>
      backend.contractHeaderValue(contractSha);

  /// COSE_Sign1 or COSE_Mac0.
  CoseMode get mode;

  /// The `Content-Type` (and `Accept`) of sealed bodies.
  String get mediaType;

  /// The signer's 8-byte `kid`.
  Uint8List get kid;

  /// Seals [payload] (already CBOR) as the request [binding] describes.
  /// Resolves to the bytes to send.
  Future<Uint8List> sealRequest(Uint8List payload, CallBinding binding);

  /// Opens the response [body] to [sealedRequest] (the exact bytes that were
  /// sent), which came back with HTTP [status].
  ///
  /// Throws [CoseRejected] for every failed check, whatever failed.
  Future<Opened> openResponse(
    Uint8List body, {
    required CallBinding binding,
    required Uint8List sealedRequest,
    required int status,
  });
}

/// Pins `iat` and `cti` so the shared vectors reproduce. **Tests only**: a
/// fixed `cti` is a replayed request.
final class SealPin {
  /// Pins [iat] (Unix seconds) and [cti] (1 to 4 or 16 bytes).
  const SealPin({required this.iat, required this.cti});

  /// `iat` in Unix seconds.
  final int iat;

  /// The `cti`.
  final Uint8List cti;
}

/// [ClientEnvelope.create] with a [pin]; for `cose_testing.dart`.
Future<ClientEnvelope> createPinnedEnvelope({
  required CoseSigner signer,
  required List<CoseServerKey> serverKeys,
  required String audience,
  required SealPin pin,
}) =>
    backend.createEnvelope(
      signer: signer,
      serverKeys: serverKeys,
      audience: audience,
      pin: pin,
    );
