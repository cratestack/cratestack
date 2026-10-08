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
import '../types.dart';

@JS('window.__cratestackCborWasmBridge.ClientEnvelope')
extension type _JsEnvelope._(JSObject _) implements JSObject {
  external static _JsEnvelope hmac(
    String alg,
    JSUint8Array secret,
    JSArray<JSObject> serverKeys,
    String audience,
    JSAny? options,
  );

  external static _JsEnvelope ed25519Seed(
    JSUint8Array seed,
    JSArray<JSObject> serverKeys,
    String audience,
    JSAny? options,
  );

  external String get mode;
  external String get mediaType;
  external JSUint8Array get kid;

  external JSPromise<JSUint8Array> sealRequest(
    JSUint8Array payload,
    JSObject binding,
  );

  external JSPromise<JSObject> openResponse(
    JSUint8Array body,
    JSObject binding,
    JSUint8Array sealedRequest,
    int status,
  );
}

@JS('window.__cratestackCborWasmBridge.ClientEnvelope')
external JSAny? get _clientEnvelopeExport;

@JS('window.__cratestackCborWasmBridge.contractHeaderValue')
external String _contractHeaderValue(JSUint8Array contractSha);

/// Loads the wasm module (the codec's, once), then builds the handle.
Future<ClientEnvelope> createEnvelope({
  required CoseSigner signer,
  required List<CoseServerKey> serverKeys,
  required String audience,
  SealPin? pin,
}) async {
  await createCborCodec();
  _requireCoseBuild();
  final keys = [for (final key in serverKeys) _serverKey(key)].toJS;
  final options = pin == null ? null : _options(pin);
  try {
    final handle = switch (signer) {
      HmacSigner() => _JsEnvelope.hmac(
          _alg(signer.alg),
          signer.secret.toJS,
          keys,
          audience,
          options,
        ),
      Ed25519Signer() => _JsEnvelope.ed25519Seed(
          signer.seed.toJS,
          keys,
          audience,
          options,
        ),
    };
    return _WebEnvelope(handle);
  } catch (error) {
    throw _exception(error);
  }
}

/// The vendored wasm may be a codec-only build (`@cratestack/cbor-web`'s, or a
/// copy hosted by the app): then `ClientEnvelope` is not exported at all, and
/// calling into it would be an obscure `undefined` error.
void _requireCoseBuild() {
  if (_clientEnvelopeExport.isUndefinedOrNull) {
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
    return _contractHeaderValue(Uint8List.fromList(contractSha).toJS);
  } catch (error) {
    throw _exception(error);
  }
}

final class _WebEnvelope implements ClientEnvelope {
  _WebEnvelope(this._handle);

  final _JsEnvelope _handle;

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
          await _handle.sealRequest(payload.toJS, _binding(binding)).toDart;
      return sealed.toDart;
    } catch (error) {
      throw _exception(error);
    }
  }

  @override
  Future<Opened> openResponse(
    Uint8List body, {
    required CallBinding binding,
    required Uint8List sealedRequest,
    required int status,
  }) async {
    try {
      final opened = await _handle
          .openResponse(
            body.toJS,
            _binding(binding),
            sealedRequest.toJS,
            status,
          )
          .toDart;
      return Opened(
        payload: (opened.getProperty('payload'.toJS) as JSUint8Array).toDart,
        kid: (opened.getProperty('kid'.toJS) as JSUint8Array).toDart,
        alg: _algFrom((opened.getProperty('alg'.toJS) as JSString).toDart),
        thumbprint:
            (opened.getProperty('thumbprint'.toJS) as JSUint8Array).toDart,
      );
    } catch (error) {
      throw _exception(error);
    }
  }
}

JSObject _object(Map<String, JSAny?> fields) {
  final object = JSObject();
  fields.forEach((name, value) => object.setProperty(name.toJS, value));
  return object;
}

JSObject _serverKey(CoseServerKey key) =>
    _object({'alg': _alg(key.alg).toJS, 'bytes': key.bytes.toJS});

JSObject _options(SealPin pin) =>
    _object({'fixedIat': pin.iat.toJS, 'fixedCti': pin.cti.toJS});

JSObject _binding(CallBinding binding) => _object({
      'method': binding.method.toJS,
      'route': binding.route.toJS,
      'pathParams': [for (final param in binding.pathParams) param.toJS].toJS,
      'query': binding.query?.toJS,
      'contractSha': binding.contractSha.toJS,
      'idempotencyKey': binding.idempotencyKey?.toJS,
      'ifMatch': binding.ifMatch?.toJS,
    });

String _alg(CoseAlg alg) => switch (alg) {
      CoseAlg.ed25519 => 'ed25519',
      CoseAlg.esp256 => 'esp256',
      CoseAlg.hmac256x64 => 'hmac256-64',
      CoseAlg.hmac256x256 => 'hmac256-256',
    };

CoseAlg _algFrom(String alg) => switch (alg) {
      'ed25519' => CoseAlg.ed25519,
      'esp256' => CoseAlg.esp256,
      'hmac256-64' => CoseAlg.hmac256x64,
      _ => CoseAlg.hmac256x256,
    };

/// The bridge throws and rejects with a plain `{ code, message }` object.
CoseException _exception(Object error) {
  if (error is CoseException) return error;
  // A JS object thrown or rejected by the bridge reaches Dart as itself in
  // dart2js and dart2wasm alike (both run in the test suite).
  // ignore: invalid_runtime_check_with_js_interop_types
  if (error is JSObject) {
    final code = error.getProperty('code'.toJS);
    final message = error.getProperty('message'.toJS);
    if (code.isA<JSString>()) {
      final text = message.isA<JSString>() ? (message as JSString).toDart : '';
      return switch ((code as JSString).toDart) {
        'rejected' => const CoseRejected(),
        _ => CoseMisuse(text),
      };
    }
  }
  return CoseMisuse('unexpected error from the web backend: $error');
}
