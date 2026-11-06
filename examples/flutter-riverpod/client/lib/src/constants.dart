import 'dart:convert';
import 'dart:typed_data';

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
const String? cratestackSchemaSha256 = '02cc93609f6e33ef57205aa2bda8a1e6ea731cfe9de9468042ff673c83a8e408';

/// Per-op contract digests (binding version 2, cratestack#1123): what a
/// signed request to each op binds into its COSE AAD, keyed like the Rust
/// client's `OP_CONTRACTS` (the RPC op id, or `<METHOD> <route template>` on
/// REST, plus `batch` for `transport rpc`), as lowercase hex. Computed by the
/// same `cratestack_core` function the Rust macros call, so the two cannot
/// disagree. The unsigned runtime does not read them; a sealer does.
const Map<String, String> cratestackOpContracts = <String, String>{
  'DELETE /boards/{id}': '22764560f39f87474a551a31a537e48d56b349ed4ed27b18583e3e0cf2a4379a',
  'DELETE /tasks/{id}': 'b80b4eedab7e6623b58e33216f67202c49c61f37abff6ebd3c6f96c49d98934c',
  'GET /boards': '89946ea5de7c870e5528bf0babd4be9ffb1322006c755bc92ac77e5cbd4d5c23',
  'GET /boards/{id}': '73effcaf09a18ae5cb6aadac429f179c9d2a3ad798097e9c03ccdfe8413096e6',
  'GET /tasks': '34d7de783bcb93d694e848db3471f9c86c5c9204984efb17373e9c2f6d04079e',
  'GET /tasks/{id}': '3f6748635dac9ff32302ee383c0e891e23e0d7f0bd96cf93edd8f2c80bdcb61a',
  'PATCH /boards/{id}': '48da7b836217709abcf4fa3253c090ee68f2b86189f8ea6240c6cf43975716ea',
  'PATCH /tasks/{id}': 'bc5b440b35082a7fa989477693522f48c1cce02101f0c95b4440fddfd394ed49',
  'POST /\$procs/clearDoneTasks': 'e61cb59a793682d54d88f5bcc3a69dd82f687798307d107cc56ecfc8429f1dde',
  'POST /\$procs/estimateFocusMinutes': 'a96af61cf2998d8b76c69baf5f907b76860f3eaa5b64cf35134192a406023e51',
  'POST /boards': '3a348af6855d701092188c2f19366818c059a59f6770e28ed7945cd84c12e394',
  'POST /tasks': '35ca0825853c3f3c577159e9c3568612df10c63d3ca6c1b9d825abdda60cb44a',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '6233d9c56994306cca7cf14e719addb198141a6aa0f3106d951212b6d31ece23';

/// The compact wire key tables of every op (`docs/design/compact-wire-keys.md`),
/// the artifact `createCompactKeys` of `package:cratestack_cbor` reads. Only a
/// client that opts into compact keys touches it:
///
/// ```dart
/// final keys = await createCompactKeys(cratestackKeyTables);
/// final hooks = CratestackCompactHooks(
///   toCompact: keys.toCompact,
///   fromCompact: keys.fromCompact,
///   selector: keys.selector,
/// );
/// ```
final Uint8List cratestackKeyTables = base64Decode(
  'gwGJhGJpZABkbmFtZQCEYmlkAGRuYW1lAIplYm9hcmQCZ2JvYXJkSWQAZGRvbmUAYmlkAGV0aXRsZQCCZ2JvYXJkSWQAgmdyZW1vdmVkAIJkYXJncwSEbm1pbnV0ZXNQZXJUYXNrAGl0YXNrQ291bnQAgmx0b3RhbE1pbnV0ZXMAgmRhcmdzB4yEc0RFTEVURSAvYm9hcmRzL3tpZH1YICJ2RWDzn4dHSlUaMaU35I1Ws0ntTtJ7GFg+PgzypDeaAAGEckRFTEVURSAvdGFza3Mve2lkfVgguAtO7at+ZiO1jjMhb2cgLEnGHzer/269PG+WxJ2Yk0wAA4RrR0VUIC9ib2FyZHNYIImUbqXefIcOVSi/C6vUvp/7EyIAbHVbySrHfly9TVwjAIIAAYRwR0VUIC9ib2FyZHMve2lkfVggc+/8rwmhiuXLaq2sQp8XnJ0qOteYCX6cA8zf6EEwluYAAYRqR0VUIC90YXNrc1ggNNfeeDvLk9aU6EjbNHH5yGxckgSYTvsXNz6cL20EB54AggADhG9HRVQgL3Rhc2tzL3tpZH1YID9nSGNdrJ/zIwLuODwOiR4j4NfwvZbPk+3Y8sgL3LYaAAOEclBBVENIIC9ib2FyZHMve2lkfVggSNp7g2IXcJq89PoyU8CQ7mjyuGGJ+OpiQMbPQ5dXFuoBAYRxUEFUQ0ggL3Rhc2tzL3tpZH1YILxbRAs1CCp/qYlHdpNSL0jBzOAhAfDJW0RA/d/TlO1JAwOEeBtQT1NUIC8kcHJvY3MvY2xlYXJEb25lVGFza3NYIOYctZp5NoLVTYj1vMOmndgvaHeYMH0QfMVuz8hCnx3eBgWEeCFQT1NUIC8kcHJvY3MvZXN0aW1hdGVGb2N1c01pbnV0ZXNYIKlq9hzymY2Ldsabr1+Qe3aGDz6qW2TPNRNBkqQGAj5RCQiEbFBPU1QgL2JvYXJkc1ggOjSK9oVdcBCSGIwvGTZoGMBZpZ9ncOKO15Rc2EwS45QBAYRrUE9TVCAvdGFza3NYIDXKCCWFPD88V3FZ6cNWhhLfEMY9PKbBudglq92mDLRKAwM=',
);

abstract final class BoardFieldNames {
  static const String id = 'id';
  static const String name = 'name';
}

abstract final class BoardIncludeNames {
  static const List<String> values = <String>[];
}

abstract final class TaskFieldNames {
  static const String id = 'id';
  static const String title = 'title';
  static const String done = 'done';
  static const String boardId = 'boardId';
}

abstract final class TaskIncludeNames {
  static const String board = 'board';
}

