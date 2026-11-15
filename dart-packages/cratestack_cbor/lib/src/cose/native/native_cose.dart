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
import '../external_signer.dart';
import '../signers.dart';
import '../types.dart';
import 'native_mapping.dart';

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
      rust_types.FlutterServerKey(alg: toRustAlg(key.alg), bytes: key.bytes),
  ];
  final options = pin == null
      ? null
      : rust_types.FlutterSealOptions(
          fixedIat: pin.iat,
          fixedCti: pin.cti,
        );
  return _guard<ClientEnvelope>(() async {
    // The keystore constructor is async (it holds a Dart callback); the
    // others are not.
    final FutureOr<rust.FlutterClientEnvelope> handle = switch (signer) {
      HmacSigner() => rust.FlutterClientEnvelope.hmac(
          alg: toRustAlg(signer.alg),
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
      ExternalSigner() => rust.FlutterClientEnvelope.esp256External(
          publicKey: signer.publicKey,
          // `invokeSigner` never throws: an error out of a `DartFn` would
          // panic the Rust side, so every end is a value.
          sign: (tbs) async => toRustOutcome(await invokeSigner(signer, tbs)),
          serverKeys: keys,
          audience: audience,
          options: options,
        ),
    };
    return _NativeEnvelope(await handle);
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
    throw coseException(error);
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
          binding: toRustBinding(binding),
        ),
      );

  @override
  Map<String, String> selectorHeaders(CallBinding binding) {
    try {
      return {
        for (final header
            in _handle.selectorHeaders(binding: toRustBinding(binding)))
          header.name: header.value,
      };
    } on rust_error.FlutterCoseError catch (error) {
      throw coseException(error);
    }
  }

  @override
  Future<Opened> openResponse(
    Uint8List body, {
    required CallBinding binding,
    required Uint8List sealedRequest,
    required int status,
    String? responsePayloadType,
  }) =>
      _guard<Opened>(() async {
        final opened = await _handle.openResponse(
          body: body,
          binding: toRustBinding(binding),
          sealedRequest: sealedRequest,
          status: status,
          responsePayloadType: responsePayloadType,
        );
        return Opened(
          payload: opened.payload,
          payloadType: opened.payloadType,
          kid: opened.kid,
          alg: fromRustAlg(opened.alg),
          thumbprint: Uint8List.fromList(opened.thumbprint),
        );
      });
}

/// Runs [body], and turns the bridge's error into a [CoseException].
Future<T> _guard<T>(FutureOr<T> Function() body) async {
  try {
    return await body();
  } on rust_error.FlutterCoseError catch (error) {
    throw coseException(error);
  }
}
