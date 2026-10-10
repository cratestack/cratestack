// Native backend: the shared vectors through flutter_rust_bridge. The VM
// platform also satisfies `dart.library.io`, hence the explicit
// `@TestOn('vm')`: see `../web_cbor_codec_test.dart` for the footgun.
@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';

import 'vectors_body.dart';

Json _read(String name) => jsonDecode(
      File('../../crates/cratestack-cose/tests/vectors/$name')
          .readAsStringSync(),
    ) as Json;

void main() {
  defineVectorTests(() async => (_read('keys.json'), _read('unary.json')));
}
