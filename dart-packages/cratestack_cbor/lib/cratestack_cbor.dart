/// `cratestack_cbor` — a native CBOR codec for CrateStack Dart/Flutter
/// clients (cratestack#563). One uniform API
/// ([CratestackCborCodec]/[createCborCodec]) auto-selecting a backend per
/// platform, mirroring `@cratestack/cbor`'s conditional-export umbrella
/// shape for JavaScript (`packages/cratestack-cbor`):
///
/// - **Native** (`dart.library.io`): flutter_rust_bridge over a vendored
///   prebuilt native library. This release vendors Linux x86_64 only — a
///   deliberate one-platform spike (cratestack#563); the full platform
///   matrix is follow-up work.
/// - **Web** (`dart.library.js_interop`): the existing
///   `cratestack-cbor-wasm` wasm-bindgen artifact (already shipped to npm
///   as `@cratestack/cbor-web`), vendored and loaded via `dart:js_interop`
///   — no new codec binding, no bundler.
///
/// Both backends produce byte-identical CBOR to `cratestack-codec-cbor`'s
/// `CborCodec` for the same input — see this package's `test/` directory
/// and `crates/cratestack-client-flutter/src/cbor/mod.rs`'s shared hex
/// fixtures.
///
/// ```dart
/// import 'package:cratestack_cbor/cratestack_cbor.dart';
///
/// final codec = await createCborCodec();
/// final bytes = codec.encodeJson('{"hello":"world"}');
/// final json = codec.decodeJson(bytes);
/// ```
library;

export 'src/cbor_codec.dart' show CratestackCborCodec, CratestackCborCodecError;
export 'src/unsupported_cbor_codec.dart'
    if (dart.library.io) 'src/native/native_cbor_codec.dart'
    if (dart.library.js_interop) 'src/web/web_cbor_codec.dart'
    show createCborCodec;
