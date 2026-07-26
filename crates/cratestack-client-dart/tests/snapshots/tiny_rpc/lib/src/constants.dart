/// Hex-encoded SHA-256 of the `.cstack` schema source this client was
/// generated from (issue #178). Sent as `x-cratestack-schema-sha` on
/// every outgoing request so the server-side drift-detection middleware
/// (`cratestack-axum::schema_fingerprint`) can warn when a client and
/// server were generated from different copies of the schema — it never
/// rejects a request, only logs. `null` when the generator wasn't given
/// a schema hash (e.g. this crate used as a library directly, bypassing
/// `cratestack generate-dart`), in which case every runtime adapter
/// omits the header entirely rather than sending an empty value.
const String? cratestackSchemaSha256 = '9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4';

abstract final class WidgetFieldNames {
  static const String id = 'id';
  static const String name = 'name';
  static const String weight = 'weight';
}

abstract final class WidgetIncludeNames {
  static const List<String> values = <String>[];
}

