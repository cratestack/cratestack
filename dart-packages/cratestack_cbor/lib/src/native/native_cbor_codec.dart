// Native backend: flutter_rust_bridge over a VENDORED prebuilt native
// library (cratestack#563 maintainer decision — vendor prebuilt binaries
// inside the package, no Rust toolchain and no network fetch imposed on
// consumers). Selected by the `dart.library.io` branch of
// `../../cratestack_cbor.dart`'s conditional export, so this file (and its
// `dart:io`/`dart:isolate`/`dart:ffi` imports) is never even parsed for a
// web compile target.
//
// This slice vendors Linux x86_64 (`blobs/linux-x64/`) and Android
// arm64-v8a/x86_64/armeabi-v7a (`blobs/android/<abi>/`) — the remaining
// platforms (macOS/Windows/iOS/Linux arm64) are still follow-up work. Any
// other platform throws a clear, actionable [UnsupportedError] rather than
// silently failing to find a library.
import 'dart:ffi' show Abi;
import 'dart:io';
import 'dart:isolate';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import '../cbor_codec.dart';
import 'rust/cbor.dart' as rust_cbor;
import 'rust/frb_generated.dart';
import 'rust/types.dart' show FlutterRuntimeError;

/// Overrides vendored-library resolution — set by CI/local verification to
/// point at a specific `.so` (e.g. to prove the vendored copy, not a
/// build-tree artifact, is what actually loads). Real consumers should
/// never need this; it exists for the same reason
/// `CRATESTACK_CLIENT_FLUTTER_NATIVE_LIB` exists in
/// `crates/cratestack-client-flutter/dart/verify_round_trip.dart`.
const _libraryOverrideEnvVar = 'CRATESTACK_CBOR_NATIVE_LIB';

class _NativeCratestackCborCodec implements CratestackCborCodec {
  const _NativeCratestackCborCodec();

  @override
  String get contentType => 'application/cbor';

  @override
  Uint8List encodeJson(String json) {
    try {
      return rust_cbor.encodeJson(json: json);
    } on FlutterRuntimeError catch (error) {
      throw CratestackCborCodecError(error.message);
    }
  }

  @override
  String decodeJson(List<int> bytes) {
    try {
      return rust_cbor.decodeJson(bytes: bytes);
    } on FlutterRuntimeError catch (error) {
      throw CratestackCborCodecError(error.message);
    }
  }
}

bool _initialized = false;

/// Initializes the flutter_rust_bridge runtime against the vendored native
/// library (once — safe to call more than once) and returns the uniform
/// codec. See [CratestackCborCodec] for the API surface.
Future<CratestackCborCodec> createCborCodec() async {
  if (!_initialized) {
    final libraryPath = await resolveVendoredLibraryPath();
    await CratestackCborRustLib.init(
      externalLibrary: ExternalLibrary.open(libraryPath),
    );
    _initialized = true;
  }
  return const _NativeCratestackCborCodec();
}

/// Resolves the path to the vendored native library for the current
/// platform. Exposed (not private) so verification harnesses can call it
/// directly to prove which file would be loaded, without also paying the
/// cost of `CratestackCborRustLib.init`.
///
/// Android uses a genuinely different resolution mechanism from
/// desktop/dev-mode Linux (see below) — its dynamic linker resolves a
/// bundled library by bare SONAME from the app's own native library
/// directory, which the Android Gradle Plugin populates at install time
/// from `android/build.gradle`'s `jniLibs.srcDirs` (the per-ABI vendored
/// libraries at `blobs/android/<abi>/`, see `just cbor-vendor-android`).
/// There is no path to compute and no dev-mode fallback to try — `dart
/// test` does not run on an Android target at all, so this is the only
/// strategy Android ever needs.
///
/// For every other supported platform (Linux, currently), tries two
/// genuinely different strategies, in order, because no single one covers
/// both a real Flutter app and `dart test`/`dart run` (cratestack#563
/// "Flutter app integration" slice):
///
/// 1. **Built Flutter app bundle.** `flutter build linux` bundles this
///    package's vendored library into `<bundle>/lib/`, next to the app
///    executable — see `linux/CMakeLists.txt` for the CMake half of this
///    contract (Flutter's `<plugin>_bundled_libraries` FFI-plugin
///    convention). A compiled Flutter AOT binary has no `.dart_tool/` and
///    no package source tree, so strategy 2 cannot resolve anything there
///    — this MUST be tried first, and is the only strategy proven against
///    a real `flutter build linux` + running the built binary.
/// 2. **Dev/test mode.** `dart test`, `dart run`, or anything else that
///    still has this package's source tree reachable (a pub cache
///    checkout, or — as in this repo — a `path:` dependency), resolved via
///    `Isolate.resolvePackageUri`. This is how the package's own `dart
///    test` suite exercised the native backend before any Flutter plugin
///    scaffolding existed, and still does (no Flutter SDK involved).
Future<String> resolveVendoredLibraryPath() async {
  final override = Platform.environment[_libraryOverrideEnvVar];
  if (override != null && override.isNotEmpty) {
    return override;
  }

  if (Platform.isAndroid) {
    if (Abi.current() != Abi.androidArm64 &&
        Abi.current() != Abi.androidX64 &&
        Abi.current() != Abi.androidArm) {
      throw UnsupportedError(
        'cratestack_cbor only vendors prebuilt Android native libraries '
        'for arm64-v8a, x86_64, and armeabi-v7a in this release '
        '(${Abi.current()} detected). Set $_libraryOverrideEnvVar to '
        'point at a self-built library to work around this in the '
        'meantime.',
      );
    }
    // No path to compute — see the doc comment above. Android's dynamic
    // linker finds this by SONAME inside the APK's extracted native
    // library directory, which android/build.gradle's jniLibs.srcDirs
    // populated from blobs/android/<abi>/ at build time.
    return 'libcratestack_client_flutter.so';
  }

  if (Abi.current() != Abi.linuxX64) {
    throw UnsupportedError(
      'cratestack_cbor only vendors a prebuilt native library for Linux '
      'x86_64 and Android (arm64-v8a, x86_64, armeabi-v7a) in this '
      'release (${Abi.current()} detected). The remaining platform matrix '
      '(macOS, Windows, iOS, Linux arm64) is follow-up work '
      '(cratestack#563). Set $_libraryOverrideEnvVar to point at a '
      'self-built library to work around this in the meantime.',
    );
  }

  final attempts = <String>[];

  final executableDir = File(Platform.resolvedExecutable).parent;
  final bundledLibrary = File(
    '${executableDir.path}/lib/libcratestack_client_flutter.so',
  );
  if (bundledLibrary.existsSync()) {
    return bundledLibrary.path;
  }
  attempts.add(
    'built Flutter app bundle at ${bundledLibrary.path} (relative to '
    'Platform.resolvedExecutable — not found)',
  );

  final packageUri = await Isolate.resolvePackageUri(
    Uri.parse('package:cratestack_cbor/cratestack_cbor.dart'),
  );
  if (packageUri != null) {
    // packageUri is .../cratestack_cbor/lib/cratestack_cbor.dart. Per RFC
    // 3986 URI merging, resolving a SINGLE ".." against it already drops
    // both the file name and the `lib/` segment, landing on the package
    // root (sibling of `blobs/`) directly — verified empirically, not just
    // by spec-reading: a naive double `resolve('..')` here overshot by one
    // level (see this PR's verification transcript).
    final packageRoot = packageUri.resolve('..');
    final libraryUri = packageRoot.resolve(
      'blobs/linux-x64/libcratestack_client_flutter.so',
    );
    final libraryFile = File.fromUri(libraryUri);
    if (libraryFile.existsSync()) {
      return libraryFile.path;
    }
    attempts.add(
      'vendored package source tree at ${libraryFile.path} (resolved via '
      'Isolate.resolvePackageUri — not found)',
    );
  } else {
    attempts.add(
      'Isolate.resolvePackageUri("package:cratestack_cbor/'
      'cratestack_cbor.dart") returned null (expected inside a compiled '
      'Flutter app; only meaningful under dart test/dart run)',
    );
  }

  throw StateError(
    'cratestack_cbor: could not locate the vendored native library. '
    'Tried:\n'
    '${attempts.map((a) => '  - $a').join('\n')}\n'
    'If you are working in the cratestack repo, this is expected on a '
    'fresh clone — the vendored artifacts are build output and are not '
    'committed (see this package\'s README). Run:\n'
    '    just cbor-vendor-native\n'
    'If you installed this package from pub.dev (or built a Flutter app '
    'depending on it), the archive/build should have shipped this file; '
    'the installation may be corrupt. Set $_libraryOverrideEnvVar to point '
    'at a specific library to work around this.',
  );
}
