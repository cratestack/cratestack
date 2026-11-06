// Web compact wire keys: the `CompactKeys` class of the vendored
// `cratestack-cbor-wasm` build, driven through `dart:js_interop`. The wasm
// module is loaded once by the codec; this file reuses its bridge object.
// A thin mapping; the tables and the rewrite are Rust's. Unlike the COSE
// class, it is in every build of the wasm, the codec-only one included.
import 'dart:js_interop';
import 'dart:typed_data';

import '../cbor_codec.dart' show CratestackCborCodecError;
import '../compact_keys.dart';
import 'web_cbor_codec.dart' show createCborCodec;

@JS('window.__cratestackCborWasmBridge.CompactKeys')
extension type _JsCompactKeys._(JSObject _) implements JSObject {
  external static _JsCompactKeys fromTables(JSUint8Array tables);

  external JSUint8Array toCompact(
    JSUint8Array named,
    String op,
    String direction,
  );

  external JSUint8Array fromCompact(
    JSUint8Array compact,
    String op,
    String direction,
  );

  external String toCompactJson(String named, String op, String direction);
  external String fromCompactJson(String compact, String op, String direction);
  external String? selector(String op);
}

@JS('window.__cratestackCborWasmBridge.CompactKeys')
external JSAny? get _compactKeysExport;

/// Loads the wasm module (the codec's, once), then loads [tables], the
/// artifact a generated client emits as `cratestackKeyTables`.
///
/// Throws [CratestackCborCodecError] if [tables] is not a key-table artifact.
Future<CompactKeys> createCompactKeys(Uint8List tables) async {
  await createCborCodec();
  if (_compactKeysExport.isUndefinedOrNull) {
    throw const CratestackCborCodecError(
      'the loaded cratestack-cbor-wasm has no CompactKeys export: use the '
      'wasm vendored by cratestack_cbor, built by `just cbor-vendor-web`',
    );
  }
  return _guard(() => _WebCompactKeys(_JsCompactKeys.fromTables(tables.toJS)));
}

// A caught JS exception's `toString()` already carries the message (see
// `_jsErrorMessage` in web_cbor_codec.dart for why it is not unwrapped).
T _guard<T>(T Function() body) {
  try {
    return body();
  } catch (error) {
    throw CratestackCborCodecError(error.toString());
  }
}

final class _WebCompactKeys implements CompactKeys {
  _WebCompactKeys(this._handle);

  final _JsCompactKeys _handle;

  @override
  Uint8List toCompact(Uint8List named, String op, String direction) =>
      _guard(() => _handle.toCompact(named.toJS, op, direction).toDart);

  @override
  Uint8List fromCompact(Uint8List compact, String op, String direction) =>
      _guard(() => _handle.fromCompact(compact.toJS, op, direction).toDart);

  @override
  String toCompactJson(String named, String op, String direction) =>
      _guard(() => _handle.toCompactJson(named, op, direction));

  @override
  String fromCompactJson(String compact, String op, String direction) =>
      _guard(() => _handle.fromCompactJson(compact, op, direction));

  @override
  String? selector(String op) => _guard(() => _handle.selector(op));
}
