// Native compact wire keys: the opaque `FlutterCompactKeys` of the vendored
// library (`crates/cratestack-client-flutter/src/cbor/compact.rs`). A thin
// mapping; the tables and the rewrite are Rust's.
import 'dart:typed_data';

import '../cbor_codec.dart' show CratestackCborCodecError;
import '../compact_keys.dart';
import 'native_cbor_codec.dart' show createCborCodec;
import 'rust/cbor/compact.dart' as rust;
import 'rust/types.dart' show FlutterRuntimeError;

/// Starts the shared runtime (the codec's, once), then loads [tables], the
/// artifact a generated client emits as `cratestackKeyTables`.
///
/// Throws [CratestackCborCodecError] if [tables] is not a key-table artifact.
Future<CompactKeys> createCompactKeys(Uint8List tables) async {
  await createCborCodec();
  return _guard(
    () => _NativeCompactKeys(rust.FlutterCompactKeys.fromTables(tables: tables)),
  );
}

T _guard<T>(T Function() body) {
  try {
    return body();
  } on FlutterRuntimeError catch (error) {
    throw CratestackCborCodecError(error.message);
  }
}

final class _NativeCompactKeys implements CompactKeys {
  _NativeCompactKeys(this._handle);

  final rust.FlutterCompactKeys _handle;

  @override
  Uint8List toCompact(Uint8List named, String op, String direction) => _guard(
    () => _handle.toCompact(named: named, op: op, direction: direction),
  );

  @override
  Uint8List fromCompact(Uint8List compact, String op, String direction) =>
      _guard(
        () => _handle.toNamed(compact: compact, op: op, direction: direction),
      );

  @override
  String toCompactJson(String named, String op, String direction) => _guard(
    () => _handle.toCompactJson(named: named, op: op, direction: direction),
  );

  @override
  String fromCompactJson(String compact, String op, String direction) =>
      _guard(
        () =>
            _handle.toNamedJson(compact: compact, op: op, direction: direction),
      );

  @override
  String? selector(String op) => _guard(() => _handle.selector(op: op));
}
