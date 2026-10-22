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
  'batch': '061926fefca5dba532237ef0c84e15ea03242b8225c2a157405e051d36f8fb92',
  'model.Widget.create': '228da84dff2be3cfebd977dd181d77cbac9c16ec29b68ff35ae2e24cb9a8fce5',
  'model.Widget.delete': '5e7fe6fb73e31541aaf4c2f4deb0ca76b5d62f2cda0f818ffd26059f87ba35a4',
  'model.Widget.get': '8e6eb5b6fa385c9ebbb8b133814d45ffa9d104a64c0d0eaba91c77c13991fba5',
  'model.Widget.list': 'a25bc81651c3a009dff343087364d4f94d0073b86601b3a6896f05a4e818adf2',
  'model.Widget.update': 'f701296a387b840990568629dc9e92499905a3224785df1686be732a2d420e8e',
  'procedure.echoName': '20341dec3ed5bb49f247effe3ced605d61f9630b3081d2e993f2a129a2025af5',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '061926fefca5dba532237ef0c84e15ea03242b8225c2a157405e051d36f8fb92';

abstract final class WidgetFieldNames {
  static const String id = 'id';
  static const String name = 'name';
  static const String weight = 'weight';
}

abstract final class WidgetIncludeNames {
  static const List<String> values = <String>[];
}

