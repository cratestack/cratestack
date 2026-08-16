// Same hex fixtures asserted by every other CBOR binding in this
// workspace (`cratestack-cbor-napi`'s
// `fixture_bytes_shared_with_the_js_cross_language_test_stay_correct`,
// `cratestack-cbor-wasm`'s wasm-bindgen tests, and
// `crates/cratestack-client-flutter/src/cbor/mod.rs`'s
// `fixture_bytes_shared_with_the_napi_and_wasm_cross_language_tests_stay_correct`).
// Asserting the SAME bytes here — independently, from Dart, against both
// this package's backends — is what proves byte-identical output across
// languages/bindings, not just internal self-consistency.
class CborFixture {
  const CborFixture(this.json, this.hex);

  final String json;
  final String hex;
}

const sharedFixtures = <CborFixture>[
  CborFixture('["cool","stack"]', '8264636f6f6c65737461636b'),
  CborFixture(
    '{"cratestack":["cool","stack"],"n":42,"ok":true}',
    'a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5',
  ),
  CborFixture('{"a":null,"b":[1,null,"x"]}', 'a26161f661628301f66178'),
];
