/// Hex-encoded SHA-256 of the `.cstack` schema source this client was
/// generated from (issue #178). Sent as `x-cratestack-schema-sha` on
/// every outgoing request so the server-side drift-detection middleware
/// (`cratestack-axum::schema_fingerprint`) can warn when a client and
/// server were generated from different copies of the schema — it never
/// rejects a request, only logs. `null` when the generator wasn't given
/// a schema hash (e.g. this crate used as a library directly, bypassing
/// `cratestack generate-dart`), in which case every runtime adapter
/// omits the header entirely rather than sending an empty value.
const String? cratestackSchemaSha256 = 'bf6114909166eaec95db3236963d9aeb1002d49c9d7e1fa07c77ed7f6df3db34';

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

