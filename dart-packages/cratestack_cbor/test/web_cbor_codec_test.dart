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

  for (final fixture in sharedFixtures) {
    test(
      'decodeJson(bytes of ${fixture.json}) matches the shared cross-binding '
      'fixture',
      () {
        expect(
          jsonDecode(codec.decodeJson(unhex(fixture.hex))),
          jsonDecode(fixture.json),
        );
      },
    );
  }

  // ADR 0019 D2: a `BigInt` is a CBOR text string (major type 3) holding
  // the canonical decimal form, so this backend must hand the decimal back
  // as the exact same string, never a number (a double cannot hold 2^53+1
  // on dart2js, and `jsonDecode` would round it).
  for (final fixture in bigIntFixtures) {
    test('BigInt ${fixture.json} is an exact text string both ways', () {
      final decimal =
          (jsonDecode(fixture.json) as Map<String, dynamic>)['amountE8'];
      expect(decimal, isA<String>());

      final bytes = codec.encodeJson(fixture.json);
      // Index 10 is the value header after `a1` and the `68` + 8-byte key;
      // its top three bits are the major type (3 = text, never 0/1 = int).
      expect(bytes[10] >> 5, 3);

      final decoded =
          jsonDecode(codec.decodeJson(bytes)) as Map<String, dynamic>;
      expect(decoded['amountE8'], decimal);
      expect(BigInt.parse(decoded['amountE8'] as String).toString(), decimal);
    });
  }

  test('malformed CBOR bytes throw CratestackCborCodecError, not a crash', () {
    expect(
      () => codec.decodeJson([0x1b]),
      throwsA(isA<CratestackCborCodecError>()),
    );
  });
}
