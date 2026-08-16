// Native backend: flutter_rust_bridge over a VENDORED prebuilt native
// library (cratestack#563 maintainer decision — vendor prebuilt binaries
// inside the package, no Rust toolchain and no network fetch imposed on
// consumers). Selected by the `dart.library.io` branch of
// `../../cratestack_cbor.dart`'s conditional export, so this file (and its
// `dart:io`/`dart:isolate`/`dart:ffi` imports) is never even parsed for a
// web compile target.
//
// This slice vendors Linux x86_64 ONLY (`blobs/linux-x64/`) — a deliberate
// one-platform spike proving the vendoring pattern before replicating it
// across the ~12 slices the full matrix needs (macOS/Windows/Android/iOS).
// Any other platform throws a clear, actionable [UnsupportedError] rather
// than silently failing to find a library.
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
Future<String> resolveVendoredLibraryPath() async {
  final override = Platform.environment[_libraryOverrideEnvVar];
  if (override != null && override.isNotEmpty) {
    return override;
  }

  if (Abi.current() != Abi.linuxX64) {
    throw UnsupportedError(
      'cratestack_cbor only vendors a prebuilt native library for Linux '
      'x86_64 in this release (${Abi.current()} detected). This is a '
      'deliberate single-platform spike (cratestack#563) — the full '
      'platform matrix (macOS, Windows, Android, iOS, Linux arm64) is '
      'follow-up work. Set $_libraryOverrideEnvVar to point at a '
      'self-built library to work around this in the meantime.',
    );
  }

  final packageUri = await Isolate.resolvePackageUri(
    Uri.parse('package:cratestack_cbor/cratestack_cbor.dart'),
  );
  if (packageUri == null) {
    throw StateError(
      'cratestack_cbor: could not resolve the package root via '
      'Isolate.resolvePackageUri — is .dart_tool/package_config.json '
      'present (did you run `dart pub get`)?',
    );
  }
  // packageUri is .../cratestack_cbor/lib/cratestack_cbor.dart. Per
  // RFC 3986 URI merging, resolving a SINGLE ".." against it already
  // drops both the file name and the `lib/` segment, landing on the
  // package root (sibling of `blobs/`) directly — verified empirically,
  // not just by spec-reading: a naive double `resolve('..')` here
  // overshot by one level (see this PR's verification transcript).
  final packageRoot = packageUri.resolve('..');
  final libraryUri = packageRoot.resolve(
    'blobs/linux-x64/libcratestack_client_flutter.so',
  );
  final libraryFile = File.fromUri(libraryUri);
  if (!libraryFile.existsSync()) {
    throw StateError(
      'cratestack_cbor: vendored native library not found at '
      '${libraryFile.path}.\n'
      'If you are working in the cratestack repo, this is expected on a '
      'fresh clone — the vendored artifacts are build output and are not '
      'committed (see this package\'s README). Run:\n'
      '    just cbor-vendor-native\n'
      'If you installed this package from pub.dev, the archive should have '
      'shipped this file; the installation may be corrupt.',
    );
  }
  return libraryFile.path;
}
