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
const String? cratestackSchemaSha256 = '13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb';

/// Per-op contract digests (binding version 2, cratestack#1123): what a
/// signed request to each op binds into its COSE AAD, keyed like the Rust
/// client's `OP_CONTRACTS` (the RPC op id, or `<METHOD> <route template>` on
/// REST, plus `batch` for `transport rpc`), as lowercase hex. Computed by the
/// same `cratestack_core` function the Rust macros call, so the two cannot
/// disagree. The unsigned runtime does not read them; a sealer does.
const Map<String, String> cratestackOpContracts = <String, String>{
  'DELETE /widget_lists/{id}': '997276e11bc8cd1ad3c58dcfdfac06bb5cb54166564eb83f1f06388e7f372058',
  'DELETE /widgets/{id}': '2657125836b3865ee0b2b80efa2cc31ec6111f5482cfa53692670db79a74ca9c',
  'GET /widget_lists': '9f491f36e77dc65b572bbe27a2bdab9b553cf4b5b243c04dead6fb960ae90b39',
  'GET /widget_lists/{id}': 'dc0980661a06aca0d727f1b7b48084d2e4e7982b4b9be20de8bcccce7e91b1c5',
  'GET /widgets': 'dd16339991c3ded012ae34ed99523c46a704c270007d44498f00105b104fd1bc',
  'GET /widgets/{id}': '18ee8e789f25a0ef04335f482788544da59b2a571747eee314785c6588993384',
  'PATCH /widget_lists/{id}': '4b7e1512b51bac369a824060acd24a5e2bcaa369fedcbea4ad02972e5ca0cb68',
  'PATCH /widgets/{id}': '503d3f0491f076c21bac96c883f2f8d52a6899880efaf58e630b26dbd3563c9d',
  'POST /\$procs/widgetCreate': 'ad3fef04562d8cb88b594dd3fd7f43fcd57ac5cd3fcffcf04a044da5cb256723',
  'POST /widget_lists': '40cfbdc71d47bf4df142ccbea5837c6102dff148be0b32800f41618d907858d0',
  'POST /widgets': 'ccbcc9377eaadbb26d6134e6a37a41c143eafa22e24a968efe6156cb331ac314',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '6f72121c70ad5acf966802e0d9077f4b98be6124d11e9d80078a543937b39288';

abstract final class WidgetFieldNames {
  static const String id = 'id';
  static const String name = 'name';
}

abstract final class WidgetIncludeNames {
  static const List<String> values = <String>[];
}

abstract final class WidgetListFieldNames {
  static const String id = 'id';
  static const String label = 'label';
}

abstract final class WidgetListIncludeNames {
  static const List<String> values = <String>[];
}

