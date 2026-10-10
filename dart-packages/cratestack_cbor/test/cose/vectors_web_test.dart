// Web backend: the same vectors through the vendored wasm build, loaded by
// `dart:js_interop`. `@TestOn('browser')` is load-bearing: see
// `../web_cbor_codec_test.dart`. Run with `dart test -p chrome`.
@TestOn('browser')
library;

import 'dart:convert';

import 'package:test/test.dart';

import 'vectors_body.dart';

void main() {
  defineVectorTests(() async {
    final channel = spawnHybridUri('vectors_hybrid.dart');
    final files = await channel.stream.cast<String>().toList();
    return (
      jsonDecode(files[0]) as Json,
      jsonDecode(files[1]) as Json,
    );
  });
}
