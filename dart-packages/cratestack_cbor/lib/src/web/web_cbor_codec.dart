// Web backend: the EXISTING `cratestack-cbor-wasm` wasm-bindgen artifact
// (crates/cratestack-cbor-wasm, already published to npm as
// `@cratestack/cbor-web` — cratestack#563 maintainer decision: reuse it,
// do not add a third binding of the same codec). Vendored here as a
// wasm-pack `--target web` build (`lib/src/web/wasm-pkg/`), loaded at
// runtime via `dart:js_interop` — no JS/npm build step, no bundler, no
// `dart:html`. Selected by the `dart.library.js_interop` branch of
// `../../cratestack_cbor.dart`'s conditional export.
//
// The genuinely novel part of this file: wasm-bindgen's `--target web`
// output is a real ES module (`import`/`export`), but `dart:js_interop`'s
// `@JS()` bindings resolve identifiers as PROPERTIES of the global object
// — ES module exports are NOT globals by default. The bridge below installs
// a tiny inline `<script type="module">` that imports the vendored module
// and stashes its exports on `window` under a private key, which the
// `@JS()` bindings then read. (The `--target no-modules` alternative was
// tried first and rejected: wasm-bindgen emits its namespace object via
// top-level `let`, which — unlike `var` — never becomes a `window`
// property, so `@JS('wasm_bindgen')`-style bindings can't see it at all.)
//
// Asset resolution: the vendored `.js`/`.wasm` pair is loaded from this
// package's own `packages/cratestack_cbor/...` URL — the standard Dart
// convention for files shipped under a package's `lib/` directory, and
// what both `flutter run -d chrome`'s dev server and `dart test -p
// chrome`'s browser runner serve automatically. This is verified working
// end to end (see `test/web_cbor_codec_test.dart`) for that dev-server
// style serving. A production `flutter build web` release bundle needs the
// consuming app to also declare these two files as Flutter assets (or
// otherwise ensure they land in the final `build/web/` output) — proper
// Flutter-web asset-bundling wiring (a `flutter:` pubspec section) is
// follow-up work, not proven by this slice. See this package's README.
import 'dart:async';
import 'dart:js_interop';
import 'dart:typed_data';

import 'package:web/web.dart' as web;

import '../cbor_codec.dart';

@JS('JSON.parse')
external JSAny? _jsonParse(String text);

@JS('JSON.stringify')
external JSString _jsonStringify(JSAny? value);

/// Private key the inline module-bridge script stashes the imported
/// module's exports under. Not exposed outside this file.
const _bridgeGlobal = '__cratestackCborWasmBridge';

@JS('window.__cratestackCborWasmBridge.init')
external JSPromise<JSAny?> _init(String wasmUrl);

@JS('window.__cratestackCborWasmBridge.contentType')
external JSString _contentType();

@JS('window.__cratestackCborWasmBridge.encode')
external JSUint8Array _encode(JSAny? value);

@JS('window.__cratestackCborWasmBridge.decode')
external JSAny? _decode(JSUint8Array bytes);

@JS('window.__cratestackCborWasmBridge.error')
external JSString? _bridgeError;

class _WebCratestackCborCodec implements CratestackCborCodec {
  const _WebCratestackCborCodec();

  @override
  String get contentType => _contentType().toDart;

  @override
  Uint8List encodeJson(String json) {
    final JSAny? value;
    try {
      value = _jsonParse(json);
    } catch (error) {
      throw CratestackCborCodecError('invalid JSON input: $error');
    }
    try {
      return _encode(value).toDart;
    } catch (error) {
      throw CratestackCborCodecError(_jsErrorMessage(error));
    }
  }

  @override
  String decodeJson(List<int> bytes) {
    final decoded = () {
      try {
        return _decode(Uint8List.fromList(bytes).toJS);
      } catch (error) {
        throw CratestackCborCodecError(_jsErrorMessage(error));
      }
    }();
    return _jsonStringify(decoded).toDart;
  }
}

// A caught JS exception's `Object.toString()` (dart2js/dart2wasm both wrap
// thrown JS values so `toString()` works) already includes the JS error's
// message — e.g. `"Error: invalid value for CBOR encode: ..."` for the
// `JsError`s `crates/cratestack-cbor-wasm/src/wasm.rs` throws. Extracting
// just `.message` would need a runtime type check against a JS interop
// type, which `dart analyze` flags as platform-inconsistent between
// dart2js and dart2wasm (`invalid_runtime_check_with_js_interop_types`) —
// not worth it for a string that's already informative.
String _jsErrorMessage(Object error) => error.toString();

bool _initialized = false;

/// Loads the vendored wasm module (once — safe to call more than once) and
/// returns the uniform codec. See [CratestackCborCodec] for the API
/// surface.
Future<CratestackCborCodec> createCborCodec() async {
  if (!_initialized) {
    final baseUrl = Uri.base.resolve(
      'packages/cratestack_cbor/src/web/wasm-pkg/',
    );
    await _installModuleBridge(
      baseUrl.resolve('cratestack_cbor_wasm.js').toString(),
    );
    final error = _bridgeError;
    if (error != null) {
      throw StateError(
        'cratestack_cbor: failed to load the vendored wasm module from '
        '$baseUrl: ${error.toDart}. If this package is not served under '
        'the default packages/cratestack_cbor/... URL (e.g. a production '
        '`flutter build web` release bundle without an asset mapping for '
        'it), see this package\'s README for how to point at a copy you '
        'host yourself.',
      );
    }
    await _init(
      baseUrl.resolve('cratestack_cbor_wasm_bg.wasm').toString(),
    ).toDart;
    _initialized = true;
  }
  return const _WebCratestackCborCodec();
}

Future<void> _installModuleBridge(String moduleUrl) {
  final completer = Completer<void>();
  final script = web.document.createElement('script') as web.HTMLScriptElement;
  script.type = 'module';
  script.text =
      '''
    try {
      const mod = await import("$moduleUrl");
      window.$_bridgeGlobal = { init: mod.default, ...mod };
    } catch (e) {
      window.$_bridgeGlobal = { error: String(e) };
    }
    window.dispatchEvent(new Event("cratestack-cbor-wasm-bridge-ready"));
  ''';
  web.window.addEventListener(
    'cratestack-cbor-wasm-bridge-ready',
    (JSAny? _) {
      if (!completer.isCompleted) completer.complete();
    }.toJS,
  );
  web.document.head!.appendChild(script);
  return completer.future;
}
