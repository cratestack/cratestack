// VM-only: proves the NATIVE backend (flutter_rust_bridge over the
// VENDORED prebuilt library) actually works, byte-identical to the shared
// cross-binding fixtures, using the real published-package public API
// (`package:cratestack_cbor/cratestack_cbor.dart`) rather than reaching
// into `src/`.
//
// `@TestOn('vm')` mirrors `web_cbor_codec_test.dart`'s own
// `@TestOn('browser')` — see that file's doc comment for why this isn't
// decorative (the conditional export in `lib/cratestack_cbor.dart` is
// resolved by the compile target, not by which test imported it).
//
// Run with `dart test test/native_cbor_codec_test.dart` (no `-p chrome` —
// this exercises `dart.library.io`).
@TestOn('vm')
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

  test('invalid JSON input throws CratestackCborCodecError', () {
    expect(
      () => codec.encodeJson('{not valid json'),
      throwsA(isA<CratestackCborCodecError>()),
    );
  });
}
