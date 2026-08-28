// Regression proof for cratestack#794's first half: `createCborCodec()` is
// idempotent for REPEATED and CONCURRENT calls.
//
// A separate file from `native_cbor_codec_test.dart` on purpose, and from
// `native_cbor_external_init_test.dart` too: what is under test here is the
// *first* initialization in a process, so nothing else in the same file may
// have already brought the runtime up. `dart test` gives each test file its
// own isolate, which is the isolation this needs — folding these cases into
// the existing suite's `setUpAll(createCborCodec)` would make every one of
// them a no-op assertion about an already-initialized runtime.
@TestOn('vm')
library;

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:test/test.dart';

void main() {
  test('concurrent first calls share one initialization', () async {
    expect(isCborRuntimeInitialized, isFalse);

    // The case a `bool _initialized` flag cannot cover: both callers reach
    // the guard before either has finished initializing, so both see
    // "not initialized" and both call `CratestackCborRustLib.init` — the
    // second of which throws `StateError('Should not initialize
    // flutter_rust_bridge twice')`. Started without an intervening
    // `await`, so they genuinely overlap rather than running in sequence.
    final codecs = await Future.wait([
      createCborCodec(),
      createCborCodec(),
      createCborCodec(),
    ]);

    expect(isCborRuntimeInitialized, isTrue);
    for (final codec in codecs) {
      expect(codec.encodeJson('{"a":1}'), isNotEmpty);
    }
  });

  test('a later sequential call returns a working codec too', () async {
    final codec = await createCborCodec();
    expect(codec.decodeJson(codec.encodeJson('{"a":1}')), '{"a":1}');
  });
}
