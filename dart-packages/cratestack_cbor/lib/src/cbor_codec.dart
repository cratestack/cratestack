import 'dart:typed_data';

/// Uniform CBOR codec surface `cratestack_cbor` exposes on every
/// platform, regardless of which backend actually implements it
/// (flutter_rust_bridge natively, wasm-bindgen on the web — see
/// `../cratestack_cbor.dart`'s conditional export).
///
/// Mirrors `@cratestack/cbor`'s `CratestackRpcCodec` shape
/// (`packages/cratestack-cbor`), adapted to the boundary type
/// flutter_rust_bridge's native binding actually has to use: JSON text,
/// not a dynamic "any value" type — see
/// `crates/cratestack-client-flutter/src/cbor/mod.rs`'s module doc for why
/// (flutter_rust_bridge has no equivalent of napi's `serde-json` feature or
/// wasm-bindgen's `JsValue`; every bridged signature is a concrete,
/// statically-known type). The web backend normalizes to the same
/// JSON-text boundary so callers never branch on platform.
abstract interface class CratestackCborCodec {
  /// `"application/cbor"` — mirrors `CborCodec::CONTENT_TYPE`.
  String get contentType;

  /// Encodes [json] (must be valid JSON text) as CBOR bytes.
  ///
  /// Throws [CratestackCborCodecError] if [json] is not valid JSON, or if
  /// the resulting value cannot be encoded.
  Uint8List encodeJson(String json);

  /// Decodes CBOR [bytes] into JSON text.
  ///
  /// Throws [CratestackCborCodecError] if [bytes] is not valid CBOR.
  String decodeJson(List<int> bytes);
}

/// One error type for both backends, so a catch clause never has to know
/// which backend produced it. Wraps a native `FlutterRuntimeError.message`
/// or a web `Error.message` — see the two backend implementations.
final class CratestackCborCodecError implements Exception {
  const CratestackCborCodecError(this.message);

  final String message;

  @override
  String toString() => 'CratestackCborCodecError: $message';
}
