/// Hex-encoded SHA-256 of the `.cstack` schema source this client was
/// generated from (issue #178). Sent as `x-cratestack-schema-sha` on
/// every outgoing request so the server-side drift-detection middleware
/// (`cratestack-axum::schema_fingerprint`) can warn when a client and
/// server were generated from different copies of the schema — it never
/// rejects a request, only logs. `null` when the generator wasn't given
/// a schema hash (e.g. this crate used as a library directly, bypassing
/// `cratestack generate-dart`), in which case every runtime adapter
/// omits the header entirely rather than sending an empty value.
// The nullable type below is structurally correct across the space of
// possible generator configs (see the doc comment above) even though it's
// provably non-null for this one generated instance — narrowing it would
// break the no-hash case, which is a real, exercised code path.
// ignore: unnecessary_nullable_for_final_variable_declarations
const String? cratestackSchemaSha256 = '9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4';

/// Per-op contract digests (binding version 2, cratestack#1123): what a
/// signed request to each op binds into its COSE AAD, keyed like the Rust
/// client's `OP_CONTRACTS` (the RPC op id, or `<METHOD> <route template>` on
/// REST, plus `batch` for `transport rpc`), as lowercase hex. Computed by the
/// same `cratestack_core` function the Rust macros call, so the two cannot
/// disagree. The unsigned runtime does not read them; a sealer does.
const Map<String, String> cratestackOpContracts = <String, String>{
  'DELETE /widgets/{id}': 'fa4fcc1604b1a248dda5422e4817349055142f492e0da601991ab6b4f7fc37d0',
  'GET /widgets': '8fb5d4c6c0e57ae9943ccfe35d455b7e4021492ad99f10e71acc625379f12308',
  'GET /widgets/{id}': '040f17b26529134ec0234511a5835ad9af3d15f6728a54e5459ac0ad65845773',
  'PATCH /widgets/{id}': '2242eec071cd19773d5c4c0691782b0a2ae977c12e697eb343d9f27e313b429c',
  'POST /\$procs/echoName': 'ca08efdfe4184532e926c9fd91541c94d3cabec1aaa3a789bc307dd2a91ae410',
  'POST /widgets': 'aef796cfb63c2d74a8de9ce3fefff9212ca226a3968c37c9fa2feb4b79b9c00b',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '0a3e98e53e970f4d8ed97572a9a21dcec8bcd12726fa4e7144084031f62148ed';

abstract final class WidgetFieldNames {
  static const String id = 'id';
  static const String name = 'name';
  static const String weight = 'weight';
}

abstract final class WidgetIncludeNames {
  static const List<String> values = <String>[];
}

