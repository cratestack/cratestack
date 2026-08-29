// Native backend: flutter_rust_bridge over a VENDORED prebuilt native
// library (cratestack#563 maintainer decision — vendor prebuilt binaries
// inside the package, no Rust toolchain and no network fetch imposed on
// consumers). Selected by the `dart.library.io` branch of
// `../../cratestack_cbor.dart`'s conditional export, so this file (and its
// `dart:io`/`dart:ffi` imports, and `package_root.dart`'s `dart:isolate`
// one) is never even parsed for a web compile target.
//
// This slice vendors Linux x86_64 (`blobs/linux-x64/`), Android
// arm64-v8a/x86_64/armeabi-v7a (`blobs/android/<abi>/`), Windows x86_64
// (`blobs/windows-x64/`), macOS arm64+x86_64 as a universal xcframework
// (`macos/Frameworks/CratestackCborNative.xcframework`, assembled by `just
// cbor-vendor-macos` from per-arch builds under `blobs/macos-{arm64,x64}/`),
// and iOS device arm64 + universal simulator arm64/x86_64 as a second
// xcframework (`ios/Frameworks/CratestackCborNative.xcframework`, assembled
// by `just cbor-vendor-ios` from per-arch builds under
// `blobs/ios-{arm64,sim-arm64,sim-x64}/`). Linux arm64 is the one gap, and
// it is blocked upstream, not deferred: Flutter ships no arm64 Linux SDK on
// any channel — see the package README for the release-manifest evidence.
// Plain `dart test`/`dart run` is NOT a way around that (cratestack#823,
// which was filed on the opposite assumption and then measured): this
// package declares `flutter.plugin.platforms`, which obliges
// `environment.flutter`, so a standalone Dart SDK fails at version solving
// before any of the resolution below runs. The dev-mode
// `Isolate.resolvePackageUri` path is reachable on arm64 Linux only for
// someone running a third-party (distro-built) Flutter SDK. Any other
// platform throws a clear, actionable [UnsupportedError] rather than
// silently failing to find a library.
import 'dart:ffi' show Abi;
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import '../cbor_codec.dart';
import 'package_root.dart';
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

/// The in-flight-or-settled result of the first successful
/// [createCborCodec] call — see that function's doc comment for why this
/// is a memoized `Future`, not the `bool` flag it replaced
/// (cratestack#794).
Future<CratestackCborCodec>? _codecFuture;

/// Whether the native runtime backing [createCborCodec] is already up.
///
/// Reports flutter_rust_bridge's OWN state
/// (`CratestackCborRustLib.instance.initialized`), not a flag private to
/// this library, so it stays accurate even for a consumer that bootstrapped
/// the bridge itself rather than through [createCborCodec] — which is the
/// whole reason it exists (cratestack#794). Never throws, and never
/// initializes anything: a `false` here is an invitation to call
/// [createCborCodec], not an error.
bool get isCborRuntimeInitialized => CratestackCborRustLib.instance.initialized;

/// Initializes the flutter_rust_bridge runtime against the vendored native
/// library (once — safe to call more than once, concurrently or not) and
/// returns the uniform codec. See [CratestackCborCodec] for the API
/// surface.
///
/// **Idempotent by two independent mechanisms** (cratestack#794 — the
/// `bool _initialized` flag this replaced provided neither, and
/// flutter_rust_bridge's `initImpl` throws `StateError('Should not
/// initialize flutter_rust_bridge twice')` on the second attempt rather
/// than no-opping):
///
/// 1. The returned `Future` is **memoized**, so concurrent callers share
///    one initialization instead of racing to run two. A plain `bool`
///    guard cannot do this: it is only set *after* the `await`s below, so
///    two callers that both arrive before the first one finishes both see
///    `false` and both call `init`.
/// 2. The `init` itself is guarded on [isCborRuntimeInitialized], i.e. on
///    flutter_rust_bridge's own state rather than on this library's. That
///    covers the case memoization structurally cannot: a consumer that
///    already called `CratestackCborRustLib.init` through its own
///    bootstrap path, whose work this library would otherwise have no way
///    to observe.
///
/// Only a *successful* initialization is memoized — the same rule, for the
/// same reason, as the generated TypeScript RPC runtime's `resolveCodec()`
/// (`crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2`)
/// and `@cratestack/cbor-web`'s `ensureInitialized()`. A failure here is
/// usually fixable without restarting the process (vendor the library,
/// set [_libraryOverrideEnvVar]), and a memoized rejection would replay
/// the same error forever instead of letting the next call retry.
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
  if (!isCborRuntimeInitialized) {
    final libraryPath = await resolveVendoredLibraryPath();
    await CratestackCborRustLib.init(
      externalLibrary: ExternalLibrary.open(libraryPath),
    );
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
/// For every other supported platform (Linux and Windows, currently), tries
/// two genuinely different strategies, in order, because no single one
/// covers both a real Flutter app and `dart test`/`dart run` (cratestack#563
/// "Flutter app integration" slice):
///
/// 1. **Built Flutter app bundle.** `flutter build linux`/`flutter build
///    windows` bundles this package's vendored library next to the app
///    executable — `<bundle>/lib/` on Linux, directly beside the `.exe` on
///    Windows (see `linux/CMakeLists.txt` and `windows/CMakeLists.txt` for
///    the CMake half of this contract — Flutter's
///    `<plugin>_bundled_libraries` FFI-plugin convention, and
///    `windows/CMakeLists.txt`'s header comment for why the install
///    destination differs between the two). A compiled Flutter AOT binary
///    has no `.dart_tool/` and no package source tree, so strategy 2
///    cannot resolve anything there — this MUST be tried first, and is the
///    only strategy proven against a real `flutter build linux` + running
///    the built binary (Windows is UNVERIFIED — this repo's toolchain is
///    Linux-only; see docs/tooling/cratestack-cbor-development.md).
/// 2. **Dev/test mode.** `dart test`, `dart run`, `flutter test`, or
///    anything else that still has this package's source tree reachable (a
///    pub cache checkout, or — as in this repo — a `path:` dependency).
///    See [lookupPackageRoot] for the two sub-strategies this uses and why
///    `Isolate.resolvePackageUri` alone is not enough (`flutter test` does
///    not implement it — cratestack#794). This is how the package's own
///    `dart test` suite exercised the native backend before any Flutter
///    plugin scaffolding existed, and still does (no Flutter SDK
///    involved).
///
/// Windows' bare filename is deliberately never opened by relying on the
/// OS's own DLL search order (e.g. `DynamicLibrary.open('cratestack_client_
/// flutter.dll')` with no path): that would plausibly work for the built-app
/// case (the DLL sits next to the running `.exe`, which Windows searches
/// first) but silently breaks the `dart test`/dev-mode fallback, where
/// `Platform.resolvedExecutable` is `dart.exe` sitting inside the Dart SDK,
/// nowhere near this package's vendored `blobs/windows-x64/`. It would also
/// discard the debuggable "tried these exact paths" error the Linux
/// strategy above deliberately provides.
///
/// macOS is a **third, genuinely different mechanism** again — not a
/// relocated copy of either of the other two, and closer to Android's
/// "no path to compute" shape than to Linux/Windows' two-step fallback,
/// though for a different underlying reason. It resolves to the fixed
/// relative string `'CratestackCborNative.framework/CratestackCborNative'`
/// (see [_resolveMacosLibraryPath]) with **no dev-mode fallback at all** —
/// `dart test`/`dart run` cannot exercise this backend on macOS, same
/// limitation as Android, because the mechanism only works inside a real
/// built `.app` bundle (see that method's doc comment for why). Verified on
/// a real `macos-latest` runner, not assumed by analogy with the other two
/// platforms: `spike/cbor-macos-xcframework`
/// (`.github/workflows/spike-cbor-macos.yml`, cratestack#563).
///
/// iOS (cratestack#563's iOS slice) reuses macOS's exact mechanism —
/// **not** a fourth one. It resolves to the identical fixed relative
/// string `'CratestackCborNative.framework/CratestackCborNative'` (see
/// [_resolveMacosLibraryPath], shared by both platforms rather than
/// duplicated into an `_resolveIosLibraryPath`, since the underlying
/// contract is the same: CocoaPods LINKS the vendored xcframework into the
/// built app via `ios/cratestack_cbor.podspec`'s `vendored_frameworks`,
/// exactly as `macos/cratestack_cbor.podspec` does, so dyld has already
/// loaded the image by the time this runs and matches it by path suffix.
/// Same "no dev-mode fallback" limitation too: `dart test`/`dart run`
/// never produce a linked `.app`, so iOS — like macOS and Android — has no
/// `dart test` story for the native backend; `example/` and `just
/// cbor-example-verify-ios` are the only way to exercise it. The
/// difference between the two platforms is entirely inside the
/// xcframework `ios/cratestack_cbor.podspec` vendors (flat/shallow
/// frameworks, no `Versions/A/...`, no symlinks, two slices for device +
/// universal simulator — see that podspec's and `just cbor-vendor-ios`'s
/// own header comments), not in how Dart resolves it. **Unverified by this
/// repo's own CI/toolchain** in the same sense every other Apple-platform
/// slice was before its own first CI run (Linux-only dev machine, no
/// Xcode/`lipo`/`xcodebuild`/simulator); `cratestack-cbor-ios` in
/// `.github/workflows/ci.yml` is this mechanism's first real execution for
/// iOS specifically (the underlying CocoaPods-links-a-vendored-xcframework
/// behavior itself was already proven for macOS by
/// `spike/cbor-macos-xcframework`).
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

  if (Platform.isWindows) {
    if (Abi.current() != Abi.windowsX64) {
      throw UnsupportedError(
        'cratestack_cbor only vendors a prebuilt native library for '
        'Windows x86_64 in this release (${Abi.current()} detected). Set '
        '$_libraryOverrideEnvVar to point at a self-built library to work '
        'around this in the meantime.',
      );
    }
    return _resolveWindowsLibraryPath();
  }

  if (Platform.isMacOS) {
    if (Abi.current() != Abi.macosArm64 && Abi.current() != Abi.macosX64) {
      throw UnsupportedError(
        'cratestack_cbor only vendors a prebuilt native library for macOS '
        'arm64 and x86_64 (as one universal xcframework) in this release '
        '(${Abi.current()} detected). Set $_libraryOverrideEnvVar to '
        'point at a self-built library to work around this in the '
        'meantime.',
      );
    }
    return _resolveMacosLibraryPath();
  }

  if (Platform.isIOS) {
    if (Abi.current() != Abi.iosArm64 && Abi.current() != Abi.iosX64) {
      throw UnsupportedError(
        'cratestack_cbor only vendors a prebuilt native library for iOS '
        'device arm64 and simulator arm64/x86_64 (as one xcframework) in '
        'this release (${Abi.current()} detected). Set '
        '$_libraryOverrideEnvVar to point at a self-built library to work '
        'around this in the meantime.',
      );
    }
    // Same fixed-string, no-path-computation mechanism as macOS — see the
    // doc comment above for why this shares _resolveMacosLibraryPath
    // rather than a separate _resolveIosLibraryPath.
    return _resolveMacosLibraryPath();
  }

  if (Abi.current() != Abi.linuxX64) {
    throw UnsupportedError(
      'cratestack_cbor only vendors a prebuilt native library for Linux '
      'x86_64, Windows x86_64, macOS (arm64, x86_64), iOS (device arm64, '
      'simulator arm64/x86_64), and Android (arm64-v8a, x86_64, '
      'armeabi-v7a) in this release (${Abi.current()} detected). Linux '
      'arm64 is the one gap, and it is blocked upstream: Flutter publishes '
      'no arm64 Linux SDK on any channel, and this package requires the '
      'Flutter SDK to resolve at all, so plain `dart test`/`dart run` is '
      'not a way around it (cratestack#823). Regenerate the client with '
      '`--no-native-cbor` for pure-Dart `package:cbor`, or set '
      '$_libraryOverrideEnvVar to point at a self-built library.',
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

  final lookup = await lookupPackageRoot();
  attempts.addAll(lookup.attempts);
  if (lookup.root != null) {
    final libraryUri = lookup.root!.resolve(
      'blobs/linux-x64/libcratestack_client_flutter.so',
    );
    final libraryFile = File.fromUri(libraryUri);
    if (libraryFile.existsSync()) {
      return libraryFile.path;
    }
    attempts.add(
      'vendored package source tree at ${libraryFile.path} (not found)',
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

/// Windows half of [resolveVendoredLibraryPath] — split out rather than
/// interleaved with the Linux strategies above because the two path
/// conventions genuinely differ (no `lib/` subdirectory in the built-app
/// case — see `windows/CMakeLists.txt`'s header comment — and the DLL has
/// no `lib` filename prefix), not because the underlying strategy differs:
/// same two-step "built app bundle, then dev-mode source tree" order as
/// Linux, for the same reason (a compiled AOT binary has no package source
/// tree to fall back to).
Future<String> _resolveWindowsLibraryPath() async {
  final attempts = <String>[];

  // Built Flutter app bundle: `flutter build windows` installs the
  // vendored DLL directly beside the built `.exe`
  // (`build/windows/x64/runner/<Debug|Profile|Release>/`) — NOT inside a
  // `lib/` subdirectory, because Flutter's own
  // `templates/app/windows.tmpl/CMakeLists.txt.tmpl` sets
  // `INSTALL_BUNDLE_LIB_DIR` to `${CMAKE_INSTALL_PREFIX}` with no `/lib`
  // suffix (unlike Linux). See `windows/CMakeLists.txt` for the other half
  // of this contract.
  final executableDir = File(Platform.resolvedExecutable).parent;
  final bundledLibrary = File(
    '${executableDir.path}\\cratestack_client_flutter.dll',
  );
  if (bundledLibrary.existsSync()) {
    return bundledLibrary.path;
  }
  attempts.add(
    'built Flutter app bundle at ${bundledLibrary.path} (relative to '
    'Platform.resolvedExecutable — not found)',
  );

  // Dev/test mode: same [lookupPackageRoot] strategies as Linux, just
  // pointed at `blobs/windows-x64/` and the unprefixed `.dll` name.
  final lookup = await lookupPackageRoot();
  attempts.addAll(lookup.attempts);
  if (lookup.root != null) {
    final libraryUri = lookup.root!.resolve(
      'blobs/windows-x64/cratestack_client_flutter.dll',
    );
    final libraryFile = File.fromUri(libraryUri);
    if (libraryFile.existsSync()) {
      return libraryFile.path;
    }
    attempts.add(
      'vendored package source tree at ${libraryFile.path} (not found)',
    );
  }

  throw StateError(
    'cratestack_cbor: could not locate the vendored native library. '
    'Tried:\n'
    '${attempts.map((a) => '  - $a').join('\n')}\n'
    'If you are working in the cratestack repo, this is expected on a '
    'fresh clone — the vendored artifacts are build output and are not '
    'committed (see this package\'s README). Run:\n'
    '    just cbor-vendor-lib windows-x64\n'
    'If you installed this package from pub.dev (or built a Flutter app '
    'depending on it), the archive/build should have shipped this file; '
    'the installation may be corrupt. Set $_libraryOverrideEnvVar to point '
    'at a specific library to work around this.',
  );
}

/// macOS half of [resolveVendoredLibraryPath] — a **third resolution
/// mechanism**, not a relocated copy of Linux/Windows' two-step fallback or
/// Android's bare-SONAME `dlopen`, split out for the same reason the other
/// two are: the underlying contract genuinely differs, not just the path
/// string.
///
/// Returns the FIXED relative string `'CratestackCborNative.framework/
/// CratestackCborNative'` — no `File.existsSync()` probing, no
/// `Platform.resolvedExecutable`-relative computation, no dev-mode
/// `Isolate.resolvePackageUri` fallback (contrast every strategy above).
/// This looks like it should not work — a bare relative string handed to
/// `DynamicLibrary.open` with no absolute path and no working-directory
/// guarantee — but it is exactly what Flutter's own `plugin_ffi` template
/// uses for iOS/macOS, and it resolves for a specific, verifiable reason:
///
/// `macos/cratestack_cbor.podspec` declares `vendored_frameworks`, and
/// CocoaPods (Flutter's default macOS build mechanism — see that podspec's
/// header comment for why this is CocoaPods, not Swift Package Manager)
/// **links** the vendored xcframework into the built app's binary, rather
/// than merely copying the `.framework` bundle into `Contents/Frameworks/`.
/// By the time this process is running, dyld has therefore already loaded
/// the `CratestackCborNative` image (at `@rpath/CratestackCborNative
/// .framework/Versions/A/CratestackCborNative` — the `@rpath`-relative
/// install name `just cbor-vendor-macos` sets via `install_name_tool -id`).
/// `DynamicLibrary.open` with a bare relative framework path matches an
/// **already-loaded** image by path suffix; it does not need to freshly
/// search a directory list the way Windows' DLL search order or a fresh
/// `dlopen` on Linux would. Verified directly, not inferred: `otool -L` on
/// the spike's built app binary
/// (`spike/cbor-macos-xcframework`/`.github/workflows/spike-cbor-macos.yml`,
/// cratestack#563) showed exactly that `@rpath/...` reference, and the
/// built app printed the same round-trip marker every other platform does.
///
/// This is also **why there is no dev-mode fallback**: the mechanism only
/// exists inside a real built `.app` bundle where CocoaPods actually linked
/// the framework. `dart test`/`dart run` never produce that — there is no
/// Xcode build, no linking step, nothing for dyld to have already loaded —
/// so, like Android, macOS has no `dart test` story for the native backend
/// at all; `example/` (a real Flutter app) and `just
/// cbor-example-verify-macos` are the only way to exercise this branch.
///
/// **Also iOS's resolution function**, not macOS-specific despite the
/// name (kept rather than renamed to `_resolveApplePlatformLibraryPath` —
/// the doc comment on [resolveVendoredLibraryPath] is what a reader
/// following the iOS branch actually lands on first, and it explains the
/// sharing there). The returned string is identical for both platforms,
/// which is not a coincidence but two different bundle layouts converging
/// on the same relative path: on macOS it matches the top-level
/// `CratestackCborNative` symlink (one of the three mandatory symlinks —
/// see `just cbor-vendor-macos`'s header comment — which itself resolves
/// through `Versions/Current/` to the real binary); on iOS, a flat/shallow
/// bundle, that same relative path names the real binary directly, with no
/// symlink indirection at all (see `just cbor-vendor-ios`'s header
/// comment). Both are `@rpath`-relative-linked into the built app by
/// CocoaPods, so dyld's already-loaded-image matching works identically
/// either way.
Future<String> _resolveMacosLibraryPath() async {
  return 'CratestackCborNative.framework/CratestackCborNative';
}
