// The `dart:js_interop` view of the vendored `cratestack-cbor-wasm` build's
// COSE exports (its `cose` feature).
import 'dart:js_interop';

@JS('window.__cratestackCborWasmBridge.ClientEnvelope')
extension type JsEnvelope._(JSObject _) implements JSObject {
  external static JsEnvelope hmac(
    String alg,
    JSUint8Array secret,
    JSArray<JSObject> serverKeys,
    String audience,
    JSAny? options,
  );

  external static JsEnvelope ed25519Seed(
    JSUint8Array seed,
    JSArray<JSObject> serverKeys,
    String audience,
    JSAny? options,
  );

  /// `ClientEnvelope.external`: `sign` is `(toBeSigned) => Promise<Uint8Array
  /// | { outcome, message? }>`. Renamed because `external` is a Dart keyword.
  @JS('external')
  external static JsEnvelope externalSigner(
    String alg,
    JSUint8Array publicKey,
    JSFunction sign,
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
    String? responsePayloadType,
  );

  external JSObject selectorHeaders(JSObject binding);
}

@JS('window.__cratestackCborWasmBridge.ClientEnvelope')
external JSAny? get clientEnvelopeExport;

@JS('Object.keys')
external JSArray<JSString> jsKeys(JSObject object);

@JS('window.__cratestackCborWasmBridge.contractHeaderValue')
external String jsContractHeaderValue(JSUint8Array contractSha);
