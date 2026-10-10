// The native COSE backend: flutter_rust_bridge over the vendored library
// that also carries the codec. Every type below is a thin mapping; the
// crypto, the canonical query and the AAD are `cratestack-cose`'s.
import 'dart:async';
import 'dart:typed_data';

import '../../native/native_cbor_codec.dart'
    show createCborCodec, isCborRuntimeInitialized;
import '../../native/rust/cose.dart' as rust_cose;
import '../../native/rust/cose/envelope.dart' as rust;
import '../../native/rust/cose/error.dart' as rust_error;
import '../../native/rust/cose/types.dart' as rust_types;
import '../cose_api.dart';
import '../errors.dart';
import '../types.dart';

/// Starts the shared runtime (the codec's, once), then builds the handle.
Future<ClientEnvelope> createEnvelope({
  required CoseSigner signer,
  required List<CoseServerKey> serverKeys,
  required String audience,
  SealPin? pin,
}) async {
  await createCborCodec();
  final keys = [
    for (final key in serverKeys)
      rust_types.FlutterServerKey(alg: _alg(key.alg), bytes: key.bytes),
  ];
  final options = pin == null
      ? null
      : rust_types.FlutterSealOptions(
          fixedIat: pin.iat,
          fixedCti: pin.cti,
        );
  return _guard<ClientEnvelope>(() {
    final handle = switch (signer) {
      HmacSigner() => rust.FlutterClientEnvelope.hmac(
          alg: _alg(signer.alg),
          secret: signer.secret,
          serverKeys: keys,
          audience: audience,
          options: options,
        ),
      Ed25519Signer() => rust.FlutterClientEnvelope.ed25519Seed(
          seed: signer.seed,
          serverKeys: keys,
          audience: audience,
          options: options,
        ),
    };
    return _NativeEnvelope(handle);
  });
}

String contractHeaderValue(Uint8List contractSha) {
  if (!isCborRuntimeInitialized) {
    throw StateError(
      'cratestack_cbor: start the backend first '
      '(await ClientEnvelope.create(...) or createCborCodec()).',
    );
  }
  try {
    return rust_cose.coseContractHeaderValue(contractSha: contractSha);
  } on rust_error.FlutterCoseError catch (error) {
    throw _exception(error);
  }
}

final class _NativeEnvelope implements ClientEnvelope {
  _NativeEnvelope(this._handle);

  final rust.FlutterClientEnvelope _handle;

  @override
  CoseMode get mode => switch (_handle.mode) {
        rust_types.FlutterCoseMode.sign1 => CoseMode.sign1,
        rust_types.FlutterCoseMode.mac0 => CoseMode.mac0,
      };

  @override
  String get mediaType => _handle.mediaType;

  @override
  Uint8List get kid => _handle.kid;

  @override
  Future<Uint8List> sealRequest(Uint8List payload, CallBinding binding) =>
      _guard<Uint8List>(
        () => _handle.sealRequest(
          payload: payload,
          binding: _binding(binding),
        ),
      );

  @override
  Future<Opened> openResponse(
    Uint8List body, {
    required CallBinding binding,
    required Uint8List sealedRequest,
    required int status,
  }) =>
      _guard<Opened>(() async {
        final opened = await _handle.openResponse(
          body: body,
          binding: _binding(binding),
          sealedRequest: sealedRequest,
          status: status,
        );
        return Opened(
          payload: opened.payload,
          kid: opened.kid,
          alg: _algFrom(opened.alg),
          thumbprint: Uint8List.fromList(opened.thumbprint),
        );
      });
}

rust_types.FlutterCallBinding _binding(CallBinding binding) =>
    rust_types.FlutterCallBinding(
      method: binding.method,
      route: binding.route,
      pathParams: binding.pathParams,
      query: binding.query,
      contractSha: binding.contractSha,
      idempotencyKey: binding.idempotencyKey,
      ifMatch: binding.ifMatch,
    );

rust_types.FlutterCoseAlg _alg(CoseAlg alg) => switch (alg) {
      CoseAlg.ed25519 => rust_types.FlutterCoseAlg.ed25519,
      CoseAlg.esp256 => rust_types.FlutterCoseAlg.esp256,
      CoseAlg.hmac256x64 => rust_types.FlutterCoseAlg.hmac25664,
      CoseAlg.hmac256x256 => rust_types.FlutterCoseAlg.hmac256256,
    };

CoseAlg _algFrom(rust_types.FlutterCoseAlg alg) => switch (alg) {
      rust_types.FlutterCoseAlg.ed25519 => CoseAlg.ed25519,
      rust_types.FlutterCoseAlg.esp256 => CoseAlg.esp256,
      rust_types.FlutterCoseAlg.hmac25664 => CoseAlg.hmac256x64,
      rust_types.FlutterCoseAlg.hmac256256 => CoseAlg.hmac256x256,
    };

/// Runs [body], and turns the bridge's error into a [CoseException].
Future<T> _guard<T>(FutureOr<T> Function() body) async {
  try {
    return await body();
  } on rust_error.FlutterCoseError catch (error) {
    throw _exception(error);
  }
}

CoseException _exception(rust_error.FlutterCoseError error) =>
    switch (error.kind) {
      rust_error.FlutterCoseErrorKind.rejected => const CoseRejected(),
      rust_error.FlutterCoseErrorKind.misuse => CoseMisuse(error.message),
      rust_error.FlutterCoseErrorKind.signerCancelled =>
        const CoseSignerCancelled(),
      rust_error.FlutterCoseErrorKind.signerTimedOut =>
        const CoseSignerTimedOut(),
      rust_error.FlutterCoseErrorKind.signerFailed =>
        CoseSignerFailed(error.message),
    };
