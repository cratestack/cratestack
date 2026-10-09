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

/// ADR 0019 D2: a `BigInt` travels as a CBOR text string (major type 3)
/// holding the canonical decimal form, so neither backend needs BigInt
/// knowledge; a text string is just a string. `hex` is the exact CBOR of
/// `{"amountE8": "<decimal>"}`, computed by hand from RFC 8949 (`a1` map of
/// one pair, `68` + the 8-byte key, then `0x60 | len` + the ASCII digits)
/// and cross-checked with an independent encoder. The same five hex
/// strings are asserted byte-identical in `cratestack-cbor-napi`'s
/// `lib.rs` (which documents every copy), `cratestack-cbor-wasm`'s
/// `value_bridge.rs`, `cratestack-client-flutter`'s `cbor/mod.rs` and
/// `dart/verify_round_trip.dart`, and the three vitest suites under
/// `packages/cratestack-cbor{,-node,-web}/tests`.
///
/// `json` is the text a generated client's `jsonEncode` hands to
/// `encodeJson`; the decimal is a JSON string, never a number.
const bigIntFixtures = <CborFixture>[
  CborFixture(
    '{"amountE8":"9223372036854775807"}',
    'a168616d6f756e7445387339323233333732303336383534373735383037',
  ),
  CborFixture(
    '{"amountE8":"-9223372036854775808"}',
    'a168616d6f756e744538742d39323233333732303336383534373735383038',
  ),
  CborFixture(
    '{"amountE8":"9007199254740993"}',
    'a168616d6f756e7445387039303037313939323534373430393933',
  ),
  CborFixture('{"amountE8":"0"}', 'a168616d6f756e7445386130'),
  CborFixture('{"amountE8":"-1"}', 'a168616d6f756e744538622d31'),
];

const sharedFixtures = <CborFixture>[
  CborFixture('["cool","stack"]', '8264636f6f6c65737461636b'),
  CborFixture(
    '{"cratestack":["cool","stack"],"n":42,"ok":true}',
    'a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5',
  ),
  CborFixture('{"a":null,"b":[1,null,"x"]}', 'a26161f661628301f66178'),
  ...bigIntFixtures,
];

/// `hex` as bytes, for the decode direction.
List<int> unhex(String hex) => [
  for (var i = 0; i < hex.length; i += 2)
    int.parse(hex.substring(i, i + 2), radix: 16),
];
