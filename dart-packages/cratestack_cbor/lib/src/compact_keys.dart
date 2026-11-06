import 'dart:typed_data';

/// The compact-wire-key tables of one client
/// (`docs/design/compact-wire-keys.md`): swaps a schema's field names for
/// integer keys on the wire, and back. Pure functions of bytes, backed by
/// the one Rust implementation (`flutter_rust_bridge` natively, the
/// `cratestack-cbor-wasm` build on the web); nothing is reimplemented in
/// Dart.
///
/// Build one with `createCompactKeys` from the `cratestackKeyTables` a
/// generated client emits, and hand its methods to the generated client's
/// `CratestackCompactHooks`:
///
/// ```dart
/// final keys = await createCompactKeys(cratestackKeyTables);
/// final hooks = CratestackCompactHooks(
///   toCompact: keys.toCompact,
///   fromCompact: keys.fromCompact,
///   selector: keys.selector,
/// );
/// ```
///
/// `direction` is the string `'request'` or `'response'` (not an enum, so
/// the method tear-offs fit the generated hook typedefs without a shared
/// type). Every failure (an unknown op, a bad direction, malformed bytes) is
/// a `CratestackCborCodecError`.
abstract interface class CompactKeys {
  /// Named CBOR bytes to compact CBOR bytes.
  Uint8List toCompact(Uint8List named, String op, String direction);

  /// Compact CBOR bytes back to named CBOR bytes.
  Uint8List fromCompact(Uint8List compact, String op, String direction);

  /// Named JSON text to compact JSON text (keys become decimal strings).
  String toCompactJson(String named, String op, String direction);

  /// Compact JSON text back to named JSON text.
  String fromCompactJson(String compact, String op, String direction);

  /// The `Cratestack-Contract` header value of [op]'s current contract (8
  /// digest bytes, unpadded base64url), or `null` for an op the tables lack.
  String? selector(String op);
}
