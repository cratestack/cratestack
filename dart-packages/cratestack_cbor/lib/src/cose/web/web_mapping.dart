// The mappings between the package's types and the wasm bridge's JS values.
import 'dart:js_interop';
import 'dart:js_interop_unsafe';

import '../cose_api.dart';
import '../errors.dart';
import '../sign_outcome.dart';
import '../types.dart';

JSObject jsObject(Map<String, JSAny?> fields) {
  final object = JSObject();
  fields.forEach((name, value) => object.setProperty(name.toJS, value));
  return object;
}

JSObject jsServerKey(CoseServerKey key) =>
    jsObject({'alg': jsAlg(key.alg).toJS, 'bytes': key.bytes.toJS});

JSObject jsOptions(SealPin pin) =>
    jsObject({'fixedIat': pin.iat.toJS, 'fixedCti': pin.cti.toJS});

JSObject jsBinding(CallBinding binding) => jsObject({
      'method': binding.method.toJS,
      'route': binding.route.toJS,
      'pathParams': [for (final param in binding.pathParams) param.toJS].toJS,
      'query': binding.query?.toJS,
      'contractSha': binding.contractSha.toJS,
      'idempotencyKey': binding.idempotencyKey?.toJS,
      'ifMatch': binding.ifMatch?.toJS,
      'payloadType': binding.payloadType?.toJS,
      'payloadAccept': binding.payloadAccept == null
          ? null
          : [for (final type in binding.payloadAccept!) type.toJS].toJS,
    });

/// What a keystore callback resolves to, as the wasm bridge reads it: the
/// signature bytes, or a tagged `{ outcome, message? }` object (a value, not
/// a rejection, so the kind survives however the engine wraps a thrown JS
/// value).
JSAny jsOutcome(SignOutcome outcome) => switch (outcome) {
      SignedOutcome() => outcome.signature.toJS,
      CancelledOutcome() => jsObject({'outcome': 'cancelled'.toJS}),
      TimedOutOutcome() => jsObject({'outcome': 'timedOut'.toJS}),
      FailedOutcome() => jsObject({
          'outcome': 'failed'.toJS,
          'message': outcome.message.toJS,
        }),
    };

String jsAlg(CoseAlg alg) => switch (alg) {
      CoseAlg.ed25519 => 'ed25519',
      CoseAlg.esp256 => 'esp256',
      CoseAlg.hmac256x64 => 'hmac256-64',
      CoseAlg.hmac256x256 => 'hmac256-256',
    };

CoseAlg algFromJs(String alg) => switch (alg) {
      'ed25519' => CoseAlg.ed25519,
      'esp256' => CoseAlg.esp256,
      'hmac256-64' => CoseAlg.hmac256x64,
      _ => CoseAlg.hmac256x256,
    };

/// The bridge throws and rejects with a plain `{ code, message }` object.
CoseException coseException(Object error) {
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
        'signer_cancelled' => const CoseSignerCancelled(),
        'signer_timed_out' => const CoseSignerTimedOut(),
        'signer_failed' => CoseSignerFailed(text),
        _ => CoseMisuse(text),
      };
    }
  }
  return CoseMisuse('unexpected error from the web backend: $error');
}
