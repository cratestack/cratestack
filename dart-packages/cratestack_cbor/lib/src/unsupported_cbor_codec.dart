// Fallback for any Dart compile target that is neither `dart.library.io`
// (native/VM/AOT) nor `dart.library.js_interop` (dart2js/dart2wasm web) —
// there is no such target as of Dart 3.x, but the conditional export in
// `../cratestack_cbor.dart` needs an unconditional default branch, and
// failing loudly and specifically here is better than a confusing "no
// such file" import error at a call site.
import 'cbor_codec.dart';

/// Always `false` — there is no backend here to have initialized. Exists
/// so the conditional export in `../cratestack_cbor.dart` can offer the
/// same public surface on every compile target (cratestack#794).
bool get isCborRuntimeInitialized => false;

Future<CratestackCborCodec> createCborCodec() {
  throw UnsupportedError(
    'cratestack_cbor: no backend is available for this Dart compile '
    'target (neither dart.library.io nor dart.library.js_interop). '
    'Supported today: native (Linux x86_64 only, via a vendored '
    'flutter_rust_bridge library) and web (via a vendored wasm-bindgen '
    'build).',
  );
}
