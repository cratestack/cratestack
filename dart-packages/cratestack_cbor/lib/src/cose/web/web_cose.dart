// The web COSE backend: the `ClientEnvelope` class of the vendored
// `cratestack-cbor-wasm` build (its `cose` feature), driven through
// `dart:js_interop`. The wasm module is loaded once by the codec; this file
// reuses its bridge object. Every type below is a thin mapping; the crypto,
// the canonical query and the AAD are `cratestack-cose`'s.
import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'dart:typed_data';

import '../../web/web_cbor_codec.dart'
    show createCborCodec, isCborRuntimeInitialized;
import '../cose_api.dart';
import '../errors.dart';
import '../external_signer.dart';
import '../signers.dart';
import '../types.dart';
import 'web_interop.dart';
import 'web_mapping.dart';

/// Loads the wasm module (the codec's, once), then builds the handle.
Future<ClientEnvelope> createEnvelope({
  required CoseSigner signer,
  required List<CoseServerKey> serverKeys,
  required String audience,
  SealPin? pin,
}) async {
  await createCborCodec();
  _requireCoseBuild();
  final keys = [for (final key in serverKeys) jsServerKey(key)].toJS;
  final options = pin == null ? null : jsOptions(pin);
  try {
    final handle = switch (signer) {
      HmacSigner() => JsEnvelope.hmac(
          jsAlg(signer.alg),
          signer.secret.toJS,
          keys,
          audience,
          options,
        ),
      Ed25519Signer() => JsEnvelope.ed25519Seed(
          signer.seed.toJS,
          keys,
          audience,
          options,
        ),
      ExternalSigner() => JsEnvelope.externalSigner(
          'esp256',
          signer.publicKey.toJS,
          // Resolves to a value for every end (never rejects), so the wasm
          // side reads one shape on both backends.
          ((JSUint8Array tbs) =>
                  invokeSigner(signer, tbs.toDart).then(jsOutcome).toJS)
              .toJS,
          keys,
          audience,
          options,
        ),
    };
    return _WebEnvelope(handle);
  } catch (error) {
    throw coseException(error);
  }
}

/// The vendored wasm may be a codec-only build (`@cratestack/cbor-web`'s, or a
/// copy hosted by the app): then `ClientEnvelope` is not exported at all, and
/// calling into it would be an obscure `undefined` error.
void _requireCoseBuild() {
  if (clientEnvelopeExport.isUndefinedOrNull) {
    throw const CoseMisuse(
      'the loaded cratestack-cbor-wasm was built without the `cose` feature '
      '(no ClientEnvelope export): use the wasm vendored by cratestack_cbor, '
      'built by `just cbor-vendor-web`',
    );
  }
}

String contractHeaderValue(Uint8List contractSha) {
  if (!isCborRuntimeInitialized) {
    throw StateError(
      'cratestack_cbor: start the backend first '
      '(await ClientEnvelope.create(...) or createCborCodec()).',
    );
  }
  _requireCoseBuild();
  try {
    return jsContractHeaderValue(Uint8List.fromList(contractSha).toJS);
  } catch (error) {
    throw coseException(error);
  }
}

final class _WebEnvelope implements ClientEnvelope {
  _WebEnvelope(this._handle);

  final JsEnvelope _handle;

  @override
  CoseMode get mode => _handle.mode == 'sign1' ? CoseMode.sign1 : CoseMode.mac0;

  @override
  String get mediaType => _handle.mediaType;

  @override
  Uint8List get kid => _handle.kid.toDart;

  @override
  Future<Uint8List> sealRequest(Uint8List payload, CallBinding binding) async {
    try {
      final sealed =
          await _handle.sealRequest(payload.toJS, jsBinding(binding)).toDart;
      return sealed.toDart;
    } catch (error) {
      throw coseException(error);
    }
  }

  @override
  Map<String, String> selectorHeaders(CallBinding binding) {
    try {
      final headers = _handle.selectorHeaders(jsBinding(binding));
      return {
        for (final name in jsKeys(headers).toDart)
          name.toDart: (headers.getProperty(name) as JSString).toDart,
      };
    } catch (error) {
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
  }) async {
    try {
      final opened = await _handle
          .openResponse(
            body.toJS,
            jsBinding(binding),
            sealedRequest.toJS,
            status,
            responsePayloadType,
          )
          .toDart;
      return Opened(
        payload: (opened.getProperty('payload'.toJS) as JSUint8Array).toDart,
        payloadType:
            (opened.getProperty('payloadType'.toJS) as JSString).toDart,
        kid: (opened.getProperty('kid'.toJS) as JSUint8Array).toDart,
        alg: algFromJs((opened.getProperty('alg'.toJS) as JSString).toDart),
        thumbprint:
            (opened.getProperty('thumbprint'.toJS) as JSUint8Array).toDart,
      );
    } catch (error) {
      throw coseException(error);
    }
  }
}
