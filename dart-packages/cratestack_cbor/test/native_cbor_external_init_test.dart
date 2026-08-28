// Regression proof for cratestack#794's second half, and the shape the
// issue actually reported: a consumer that bootstrapped
// flutter_rust_bridge ITSELF — because `flutter test` could not resolve the
// vendored library and it wrote its own loader around the env-var
// workaround — then hits `createCborCodec()` indirectly, through a
// generated `transport rpc` client that imports `package:cratestack_cbor`
// on its own. Before the fix that second initialization threw
// `StateError('Should not initialize flutter_rust_bridge twice')`.
//
// Its own file for the same isolate-isolation reason as
// `native_cbor_repeat_init_test.dart` — the external bootstrap has to be
// the first initialization in the process for the test to mean anything.
//
// This is the one test in the suite that reaches into `src/` rather than
// using the published `package:cratestack_cbor/cratestack_cbor.dart`
// surface, and deliberately so: it is standing in for consumer code that
// does exactly that, which is why the collision existed at all.
@TestOn('vm')
library;

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:cratestack_cbor/src/native/native_cbor_codec.dart'
    show resolveVendoredLibraryPath;
import 'package:cratestack_cbor/src/native/rust/frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:test/test.dart';

void main() {
  setUpAll(() async {
    expect(isCborRuntimeInitialized, isFalse);
    await CratestackCborRustLib.init(
      externalLibrary: ExternalLibrary.open(await resolveVendoredLibraryPath()),
    );
    expect(
      isCborRuntimeInitialized,
      isTrue,
      reason: 'isCborRuntimeInitialized must report flutter_rust_bridge\'s own '
          'state, not a flag private to createCborCodec — a consumer that '
          'bootstrapped the bridge itself is exactly who needs to ask',
    );
  });

  test('createCborCodec reuses an externally initialized runtime', () async {
    final codec = await createCborCodec();
    expect(codec.contentType, 'application/cbor');
    expect(codec.decodeJson(codec.encodeJson('{"a":1}')), '{"a":1}');
  });
}
