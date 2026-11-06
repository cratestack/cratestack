// Fallback for a compile target that is neither `dart.library.io` nor
// `dart.library.js_interop`; see `unsupported_cbor_codec.dart`.
import 'dart:typed_data';

import 'compact_keys.dart';

Future<CompactKeys> createCompactKeys(Uint8List tables) {
  throw UnsupportedError(
    'cratestack_cbor: no backend is available for this Dart compile '
    'target (neither dart.library.io nor dart.library.js_interop).',
  );
}
