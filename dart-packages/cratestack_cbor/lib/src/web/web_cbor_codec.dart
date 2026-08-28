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

/// The in-flight-or-settled result of the first successful
/// [createCborCodec] call — see that function's doc comment.
Future<CratestackCborCodec>? _codecFuture;

/// Whether the wasm module backing [createCborCodec] is already loaded.
///
/// Web counterpart of the native backend's identically-named getter
/// (cratestack#794), so consumer code can ask the question without
/// branching on platform. Unlike the native one there is no third-party
/// runtime to consult here — nothing but this library loads the vendored
/// module — so this reports the same flag [createCborCodec] sets.
bool get isCborRuntimeInitialized => _initialized;

/// Candidate base URLs (relative to [Uri.base]) the vendored `.js`/`.wasm`
/// pair might be served from, tried in order — cratestack#563's "Flutter
/// Web asset bundling" slice. Two genuinely different serving conventions
/// exist and neither subsumes the other:
///
/// - `packages/cratestack_cbor/...` — the plain "package: URL" convention
///   `dart test -p chrome` and `flutter run -d chrome`'s DWDS dev server
///   serve directly from the package's `lib/` source tree. No `flutter:
///   assets:` declaration is involved; this worked before this slice.
/// - `assets/packages/cratestack_cbor/...` — where a RELEASE `flutter
///   build web` actually places package assets declared in `pubspec.yaml`
///   (see this package's `flutter: assets:` section) inside
///   `build/web/assets/`. The dev server has no such `assets/` prefix, and
///   a release bundle has no special `/packages/` route — each convention
///   only exists in its own context, so both must be tried. Note this one
///   keeps the FULL path declared in `pubspec.yaml`'s `assets:` entries —
///   including the `lib/` segment the `packages/...` dev-server convention
///   strips — verified against a real `flutter build web` release output,
///   not assumed by symmetry with the dev-server URL above.
const _wasmPkgBaseUrlCandidates = [
  'packages/cratestack_cbor/src/web/wasm-pkg/',
  'assets/packages/cratestack_cbor/lib/src/web/wasm-pkg/',
];

/// Loads the vendored wasm module (once — safe to call more than once,
/// concurrently or not) and returns the uniform codec. See
/// [CratestackCborCodec] for the API surface.
///
/// The returned `Future` is memoized, so concurrent callers share one load
/// rather than racing to append two `<script>` bridges. The `bool` flag
/// alone could not do that — it is only set after the `await`s below, so
/// two callers arriving before the first finishes would both see `false`.
/// This backend's second call was never *fatal* the way the native
/// backend's was (cratestack#794 — flutter_rust_bridge rejects a second
/// `init` outright), but the race was equally real, and the two backends
/// answering "is it up?" differently would be its own trap.
///
/// Only a *successful* load is memoized, matching the native backend and
/// `@cratestack/cbor-web`'s `ensureInitialized()`: a memoized rejection
/// would replay a transient asset-loading failure forever instead of
/// letting the next call retry.
Future<CratestackCborCodec> createCborCodec() =>
    _codecFuture ??= _createCborCodec().onError<Object>((error, stackTrace) {
      // Un-memoize on failure. Hung off the future rather than written as
      // a `try`/`catch` inside `_createCborCodec` so it cannot run before
      // the `??=` above has assigned — an `onError` callback is always
      // asynchronous, whereas a `catch` body would fire synchronously for
      // anything that threw before the first `await`, and the assignment
      // would then immediately re-memoize the very failure it cleared.
      _codecFuture = null;
      Error.throwWithStackTrace(error, stackTrace);
    });

Future<CratestackCborCodec> _createCborCodec() async {
  final failures = <String>[];
  Uri? loadedFrom;
  for (final candidate in _wasmPkgBaseUrlCandidates) {
    final baseUrl = Uri.base.resolve(candidate);
    await _installModuleBridge(
      baseUrl.resolve('cratestack_cbor_wasm.js').toString(),
    );
    final error = _bridgeError;
    if (error == null) {
      loadedFrom = baseUrl;
      break;
    }
    failures.add('$baseUrl: ${error.toDart}');
  }
  if (loadedFrom == null) {
    throw StateError(
      'cratestack_cbor: failed to load the vendored wasm module. Tried:\n'
      '${failures.map((f) => '  - $f').join('\n')}\n'
      'If this package is served from neither the dev-server '
      'packages/cratestack_cbor/... URL nor a release flutter build '
      'web\'s assets/packages/cratestack_cbor/... path, see this '
      'package\'s README for how to point at a copy you host yourself.',
    );
  }
  await _init(
    loadedFrom.resolve('cratestack_cbor_wasm_bg.wasm').toString(),
  ).toDart;
  _initialized = true;
  return const _WebCratestackCborCodec();
}

Future<void> _installModuleBridge(String moduleUrl) {
  final completer = Completer<void>();
  final script = web.document.createElement('script') as web.HTMLScriptElement;
  script.type = 'module';
  script.text = '''
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
