// Dev-mode package-root resolution, shared by the Linux and Windows halves
// of `native_cbor_codec.dart`'s `resolveVendoredLibraryPath` (both need the
// same "where is this package's source tree, so `blobs/<platform>/` is a
// sibling of it" answer; only the leaf path differs).
//
// Split into its own file rather than inlined twice for the reason the
// second strategy below exists at all: `Isolate.resolvePackageUri` is NOT
// available in every runtime that can load a `.so`, and the fallback that
// covers the gap is more than a one-liner (cratestack#794).
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

const _packageName = 'cratestack_cbor';

final _packageLibraryUri =
    Uri.parse('package:$_packageName/$_packageName.dart');

/// The outcome of [lookupPackageRoot]: the package root URI if one was
/// found, plus the human-readable trail of what was tried on the way.
///
/// [attempts] is populated even on success (it holds whatever was tried
/// *before* the strategy that worked) so callers can fold it into the
/// "tried these exact paths" [StateError] they raise when the library file
/// itself is then missing under an otherwise correctly-resolved root.
class PackageRootLookup {
  const PackageRootLookup(this.root, this.attempts);

  /// Directory URI of this package's root — the sibling of `blobs/` —
  /// always with a trailing slash, so `root.resolve('blobs/...')` keeps
  /// the last segment instead of replacing it. Null when no strategy
  /// resolved it.
  final Uri? root;

  /// Failure notes, one per strategy that did not produce [root].
  final List<String> attempts;
}

/// Resolves this package's root directory in dev/test mode, trying two
/// strategies in order.
///
/// 1. **`Isolate.resolvePackageUri`.** The original (and still primary)
///    mechanism — works under `dart test`, `dart run`, and anything else
///    running on the Dart VM with a package resolution config attached to
///    the isolate.
/// 2. **`.dart_tool/package_config.json`, read directly.** Covers the
///    runtimes where strategy 1 does not merely return null but *throws*:
///    `flutter test` runs on `flutter_tester`, where
///    `Isolate.resolvePackageUriSync` — which `resolvePackageUri`
///    delegates to — is unimplemented and raises `UnsupportedError:
///    Unsupported operation: Isolate.resolvePackageUriSync`
///    (cratestack#794). The same `package_config.json` that strategy 1
///    would have consulted is still on disk there; nothing but the isolate
///    API to read it is missing.
///
/// Strategy 2 walks up from [Directory.current] rather than looking only
/// beside it, so it also resolves from a test or tool invoked with a
/// nested working directory, and from a pub workspace whose
/// `package_config.json` lives at the workspace root rather than in the
/// member package. A config that exists but does not list this package is
/// not the end of the search — the walk continues outward, since an outer
/// workspace config may well list it.
///
/// Returns rather than throws when nothing resolves: the caller has more
/// context (which platform, which leaf path, which `just` recipe to
/// suggest) and composes the actual error.
Future<PackageRootLookup> lookupPackageRoot() async {
  final attempts = <String>[];

  try {
    final packageUri = await Isolate.resolvePackageUri(_packageLibraryUri);
    if (packageUri != null) {
      // packageUri is .../cratestack_cbor/lib/cratestack_cbor.dart. Per RFC
      // 3986 URI merging, resolving a SINGLE ".." against it already drops
      // both the file name and the `lib/` segment, landing on the package
      // root (sibling of `blobs/`) directly — verified empirically, not
      // just by spec-reading: a naive double `resolve('..')` here overshot
      // by one level.
      return PackageRootLookup(packageUri.resolve('..'), attempts);
    }
    attempts.add(
      'Isolate.resolvePackageUri("$_packageLibraryUri") returned null '
      '(expected inside a compiled Flutter app; only meaningful under dart '
      'test/dart run)',
    );
  } on UnsupportedError catch (error) {
    attempts.add(
      'Isolate.resolvePackageUri("$_packageLibraryUri") is unimplemented in '
      'this runtime ($error) — expected under `flutter test`',
    );
  }

  final fromConfig = packageRootFromPackageConfig();
  attempts.addAll(fromConfig.attempts);
  return PackageRootLookup(fromConfig.root, attempts);
}

/// Strategy 2 of [lookupPackageRoot] on its own — reading
/// `.dart_tool/package_config.json` directly, with no `dart:isolate`
/// involvement whatsoever.
///
/// Exposed separately (not just as a private branch of [lookupPackageRoot])
/// because it is the half that cannot be reached from a runtime where
/// strategy 1 works, which is every runtime this repo's own test suite runs
/// in. Calling it directly is the only way to prove it resolves at all,
/// short of adding a `flutter test` job whose sole purpose is to fail
/// differently.
PackageRootLookup packageRootFromPackageConfig() {
  final attempts = <String>[];
  return PackageRootLookup(_rootFromPackageConfig(attempts), attempts);
}

Uri? _rootFromPackageConfig(List<String> attempts) {
  final searched = <String>[];
  for (var directory = Directory.current.absolute;;) {
    final config = File(
      '${directory.path}${Platform.pathSeparator}.dart_tool'
      '${Platform.pathSeparator}package_config.json',
    );
    if (config.existsSync()) {
      final root = _packageRootFrom(config, attempts);
      if (root != null) {
        return root;
      }
      searched.add(config.path);
    }
    final parent = directory.parent;
    if (parent.path == directory.path) {
      attempts.add(
        searched.isEmpty
            ? 'no .dart_tool/package_config.json in ${Directory.current.path} '
                'or any ancestor directory'
            : 'package "$_packageName" is not listed in '
                '${searched.join(', ')}',
      );
      return null;
    }
    directory = parent;
  }
}

Uri? _packageRootFrom(File config, List<String> attempts) {
  final Object? decoded;
  try {
    decoded = jsonDecode(config.readAsStringSync());
  } on Object catch (error) {
    // A malformed or unreadable config is worth reporting but not worth
    // failing on — an ancestor config may still answer the question.
    attempts.add('${config.path} could not be read as JSON ($error)');
    return null;
  }
  if (decoded is! Map<String, Object?>) {
    return null;
  }
  final packages = decoded['packages'];
  if (packages is! List) {
    return null;
  }
  for (final package in packages) {
    if (package is! Map<String, Object?> || package['name'] != _packageName) {
      continue;
    }
    final rootUri = package['rootUri'];
    if (rootUri is! String) {
      return null;
    }
    // Per the package-config spec a relative `rootUri` is resolved against
    // the DIRECTORY CONTAINING package_config.json (i.e. `.dart_tool/`),
    // not against the repository root — hence `config.parent.uri`, which
    // Dart already gives us with the trailing slash `resolve` needs.
    final root = config.parent.uri.resolve(rootUri);
    return root.path.endsWith('/') ? root : root.replace(path: '${root.path}/');
  }
  return null;
}
