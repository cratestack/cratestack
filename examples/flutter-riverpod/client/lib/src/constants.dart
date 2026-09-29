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
const String? cratestackSchemaSha256 = '898f7c4524089409c9d23e952fb9cef04ffe3879193ddb32ea11abcf16ef33f3';

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

