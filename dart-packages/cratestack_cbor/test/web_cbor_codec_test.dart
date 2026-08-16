// Browser-only: proves the WEB backend (dart:js_interop driving a
// vendored wasm-bindgen build) actually works, byte-identical to the
// shared cross-binding fixtures, using the real published-package public
// API (`package:cratestack_cbor/cratestack_cbor.dart`) rather than
// reaching into `src/`.
//
// `@TestOn('browser')` is load-bearing, not decorative: the conditional
// export in `lib/cratestack_cbor.dart` is resolved by the CURRENT COMPILE
// TARGET, not by which test file did the importing. Verified the hard way
// — without this annotation, a plain `dart test` (default `vm` platform,
// which also satisfies `dart.library.io`) silently compiled this exact
// file against the NATIVE backend and reported every assertion passing,
// having exercised zero `dart:js_interop` code. `@TestOn('browser')` makes
// `dart test` (no `-p`) skip this file with an explicit "in 0 of 1
// platform" note instead of silently mis-testing it — the same footgun
// `package:http`'s own browser-only tests guard against.
//
// Run with `dart test -p chrome test/web_cbor_codec_test.dart`.
@TestOn('browser')
library;

import 'dart:convert';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:test/test.dart';

import 'shared_fixtures.dart';

String _hex(List<int> bytes) =>
    bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

void main() {
  late CratestackCborCodec codec;

  setUpAll(() async {
    codec = await createCborCodec();
  });

  test('contentType matches the codec constant', () {
    expect(codec.contentType, 'application/cbor');
  });

  test('encodeJson -> decodeJson round-trips a JSON value', () {
    const input = '{"cratestack":["cool","stack"],"n":42}';
    final bytes = codec.encodeJson(input);
    final decoded = codec.decodeJson(bytes);
    expect(jsonDecode(decoded), jsonDecode(input));
  });

  for (final fixture in sharedFixtures) {
    test(
      'encodeJson(${fixture.json}) matches the shared cross-binding fixture',
      () {
        expect(_hex(codec.encodeJson(fixture.json)), fixture.hex);
      },
    );
  }

  test('malformed CBOR bytes throw CratestackCborCodecError, not a crash', () {
    expect(
      () => codec.decodeJson([0x1b]),
      throwsA(isA<CratestackCborCodecError>()),
    );
  });
}
