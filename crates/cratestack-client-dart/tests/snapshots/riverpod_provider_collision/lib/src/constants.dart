/// Hex-encoded SHA-256 of the `.cstack` schema source this client was
/// generated from (issue #178). Sent as `x-cratestack-schema-sha` on
/// every outgoing request so the server-side drift-detection middleware
/// (`cratestack-axum::schema_fingerprint`) can warn when a client and
/// server were generated from different copies of the schema — it never
/// rejects a request, only logs. `null` when the generator wasn't given
/// a schema hash (e.g. this crate used as a library directly, bypassing
/// `cratestack generate-dart`), in which case every runtime adapter
/// omits the header entirely rather than sending an empty value.
const String? cratestackSchemaSha256 = '13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb';

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

