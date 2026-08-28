// Proof for cratestack#794's third part: the `.dart_tool/
// package_config.json` fallback that lets `resolveVendoredLibraryPath`
// resolve under `flutter test`, where `Isolate.resolvePackageUri` does not
// merely return null but throws `UnsupportedError`.
//
// Tests `packageRootFromPackageConfig()` DIRECTLY rather than through
// `lookupPackageRoot()`, because under `dart test` — the only runner this
// package's suite has — strategy 1 always succeeds and the fallback would
// never be reached. Asserting through the front door here would produce a
// green test that exercises none of the new code.
@TestOn('vm')
library;

import 'dart:io';

import 'package:cratestack_cbor/src/native/package_root.dart';
import 'package:test/test.dart';

void main() {
  test('resolves this package root from .dart_tool/package_config.json', () {
    final lookup = packageRootFromPackageConfig();

    expect(
      lookup.root,
      isNotNull,
      reason: 'attempts: ${lookup.attempts.join('; ')}',
    );
    expect(
      lookup.root!.path,
      endsWith('/'),
      reason:
          'a directory URI without a trailing slash makes resolve("blobs/…") '
          'replace the last segment instead of appending to it',
    );
    // The root is the sibling of `blobs/`, i.e. the directory holding this
    // package's pubspec — the same anchor the Isolate strategy lands on.
    expect(
      File.fromUri(lookup.root!.resolve('pubspec.yaml')).existsSync(),
      isTrue,
      reason: 'resolved root was ${lookup.root}',
    );
    expect(
      File.fromUri(
        lookup.root!.resolve('lib/src/native/package_root.dart'),
      ).existsSync(),
      isTrue,
      reason: 'resolved root was ${lookup.root}',
    );
  });

  test('resolves identically from a nested working directory', () {
    // `.dart_tool/` sits beside the pubspec, so a runner invoked from a
    // subdirectory only resolves if the search walks upward.
    final fromPackageRoot = packageRootFromPackageConfig().root;
    final original = Directory.current;
    try {
      Directory.current = Directory('${original.path}/lib/src/native');
      expect(packageRootFromPackageConfig().root, fromPackageRoot);
    } finally {
      Directory.current = original;
    }
  });
}
