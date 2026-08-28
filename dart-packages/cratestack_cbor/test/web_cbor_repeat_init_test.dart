// Web counterpart of `native_cbor_repeat_init_test.dart` (cratestack#794).
//
// The web backend's second call was never fatal the way the native one was
// — there is no flutter_rust_bridge here to reject a second `init` — but
// the race that made the native one fatal is identical on both sides, and
// so is the memoization that fixes it. Without a test here, the two
// backends could drift back apart silently, since only one of them fails
// loudly when it does.
//
// Its own file for the same reason its native sibling is: what is under
// test is the FIRST load in the page, so nothing else may have already
// run it. `dart test -p chrome` gives each test file its own suite.
@TestOn('browser')
library;

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:test/test.dart';

void main() {
  test('concurrent first calls share one module load', () async {
    expect(isCborRuntimeInitialized, isFalse);

    final codecs = await Future.wait([
      createCborCodec(),
      createCborCodec(),
      createCborCodec(),
    ]);

    expect(isCborRuntimeInitialized, isTrue);
    for (final codec in codecs) {
      expect(codec.decodeJson(codec.encodeJson('{"a":1}')), '{"a":1}');
    }
  });
}
