/// Marks a generated data class as needing a fluent builder.
///
/// Applied by CrateStack's Dart client generator to every emitted data class
/// — models, `Create{Model}Input`, `Update{Model}Input`, `{Model}Where`,
/// `{Model}OrderByClause`, `{Model}FindMany`, `type` blocks and per-procedure
/// argument classes. `package:cratestack_builder` turns each one into a
/// `part '<file>.builder.dart'` containing a `{Class}Builder`.
///
/// It is also usable on hand-written classes: nothing about the generator
/// assumes the annotated class came from a `.cstack` schema.
///
/// ## Why this carries a parameter at all
///
/// Almost everything the generator needs is recoverable from the annotated
/// class itself via the analyzer — which fields exist, their types, which are
/// required (`isRequiredNamed` on the constructor parameter), which are lists
/// and what their element type is. None of that is threaded through here.
///
/// [listDefaults] is the one exception, and it is not an oversight. A
/// projection model's list field and a patch input's list field emit
/// *byte-identical* Dart:
///
/// ```dart
/// this.tags                 // optional named parameter
/// final List<String>? tags; // nullable field
/// ```
///
/// Yet they must build differently: an unset list on a model becomes `[]`,
/// while an unset list on a patch input must stay `null` so "untouched" is
/// distinguishable from "explicitly set to empty". Since the two are
/// indistinguishable in the source the generator reads, the distinction has
/// to be supplied by whoever emits the annotation. Inferring it from
/// nullability — the obvious-looking shortcut — is wrong, and produces
/// builders that are self-consistent and quietly disagree with the schema.
class CratestackBuilder {
  /// Whether an unset list field builds as an empty list rather than `null`.
  ///
  /// `true` for every class kind except patch inputs (`Update{Model}Input`),
  /// which pass `false`.
  final bool listDefaults;

  const CratestackBuilder({this.listDefaults = true});
}
